//! Deployment logic.

pub mod goal;

pub use goal::Goal;

pub mod limits;
pub use limits::{EvaluationNodeLimit, ParallelismLimit};

pub mod options;
pub use options::{EvaluatorType, Options};

pub mod activate;

pub mod build;

pub mod executors;
use executors::{ChunkedExecutor, DeploymentExecutor, StreamingExecutor};

use std::collections::HashMap;
use std::mem;
use std::sync::Arc;

use super::NixFlags;
use crate::job::{JobHandle, JobMonitor, JobState, JobType};
use crate::progress::Sender as ProgressSender;
use crate::util;
use futures::future::join_all;

use super::{
    key::Key, Hive, Host, NaviError, NaviResult, NodeConfig, NodeName, Profile, ProfileDerivation,
};

/// A deployment.
pub type DeploymentHandle = Arc<Deployment>;

/// A map of target nodes.
pub type TargetNodeMap = HashMap<NodeName, TargetNode>;

/// A deployment.
#[derive(Debug)]
pub struct Deployment {
    /// The configuration.
    pub(crate) hive: Hive,

    /// The goal of this deployment.
    pub(crate) goal: Goal,

    /// Deployment options.
    pub(crate) options: Options,

    /// Options passed to Nix invocations.
    pub(crate) nix_options: NixFlags,

    /// Handle to send messages to the ProgressOutput.
    pub(crate) progress: Option<ProgressSender>,

    /// Handles to the deployment targets.
    pub(crate) targets: HashMap<NodeName, TargetNode>,

    /// Parallelism limit.
    pub(crate) parallelism_limit: ParallelismLimit,

    /// Evaluation limit.
    pub(crate) evaluation_node_limit: EvaluationNodeLimit,

    /// Whether it was executed.
    executed: bool,
}

/// Handle to a target node.
#[derive(Debug)]
pub struct TargetNode {
    /// Name of the node.
    name: NodeName,

    /// The host to deploy to.
    host: Option<Box<dyn Host>>,

    /// The config.deployment values of the node.
    pub config: NodeConfig,
}

impl TargetNode {
    pub fn new(name: NodeName, host: Option<Box<dyn Host>>, config: NodeConfig) -> Self {
        Self { name, host, config }
    }

    pub fn into_host(self) -> Option<Box<dyn Host>> {
        self.host
    }
}

impl Deployment {
    /// Creates a new deployment.
    pub fn new(
        hive: Hive,
        targets: TargetNodeMap,
        goal: Goal,
        progress: Option<ProgressSender>,
    ) -> Self {
        Self {
            hive,
            goal,
            options: Options::default(),
            nix_options: NixFlags::default(),
            progress,
            targets,
            parallelism_limit: ParallelismLimit::default(),
            evaluation_node_limit: EvaluationNodeLimit::default(),
            executed: false,
        }
    }

    /// Executes the deployment.
    ///
    /// If a ProgressSender is supplied, then this should be run in parallel
    /// with its `run_until_completion()` future.
    pub async fn execute(mut self) -> NaviResult<()> {
        if self.executed {
            return Err(NaviError::DeploymentAlreadyExecuted);
        }

        self.executed = true;

        let (mut monitor, meta) = JobMonitor::new(self.progress.clone());

        if let Some(width) = util::get_label_width(&self.targets) {
            monitor.set_label_width(width);
        }

        let nix_options = self.hive.nix_flags_with_builders().await?;
        self.nix_options = nix_options;

        if self.goal == Goal::UploadKeys {
            // Just upload keys
            let targets = mem::take(&mut self.targets);
            let deployment = DeploymentHandle::new(self);
            let meta_future = meta.run(|meta| async move {
                let mut futures = Vec::new();

                for target in targets.into_values() {
                    futures.push(deployment.upload_keys_to_node(meta.clone(), target));
                }

                join_all(futures)
                    .await
                    .into_iter()
                    .collect::<NaviResult<Vec<()>>>()?;

                Ok(())
            });

            let (result, _) = tokio::join!(meta_future, monitor.run_until_completion(),);

            result?;

            Ok(())
        } else {
            // Do the whole eval-build-deploy flow
            let targets = mem::take(&mut self.targets);
            let deployment = DeploymentHandle::new(self);
            let meta_future = meta.run(|meta| async move {
                let executor: Box<dyn DeploymentExecutor> = match deployment.options.evaluator {
                    EvaluatorType::Chunked => Box::new(ChunkedExecutor),
                    EvaluatorType::Streaming => {
                        tracing::warn!("Streaming evaluation is an experimental feature");
                        Box::new(StreamingExecutor)
                    }
                };
                executor.execute(&deployment, meta.clone(), targets).await
            });

            let (result, _) = tokio::join!(meta_future, monitor.run_until_completion(),);

            result?;

            Ok(())
        }
    }

    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    pub fn set_parallelism_limit(&mut self, limit: ParallelismLimit) {
        self.parallelism_limit = limit;
    }

    pub fn set_evaluation_node_limit(&mut self, limit: EvaluationNodeLimit) {
        self.evaluation_node_limit = limit;
    }

    /// Evaluates a set of nodes, returning their corresponding store derivations.
    pub(crate) async fn evaluate_nodes(
        self: &DeploymentHandle,
        parent: JobHandle,
        nodes: Vec<NodeName>,
    ) -> NaviResult<HashMap<NodeName, ProfileDerivation>> {
        let job = parent.create_job(JobType::Evaluate, nodes.clone())?;

        job.run_waiting(|job| async move {
            // Wait for eval limit
            let permit = self.parallelism_limit.evaluation.acquire().await.unwrap();
            job.state(JobState::Running)?;

            let result = self.hive.eval_selected(&nodes, Some(job.clone())).await;

            drop(permit);
            result
        })
        .await
    }

    /// Only uploads keys to a node.
    pub(crate) async fn upload_keys_to_node(
        self: &DeploymentHandle,
        parent: JobHandle,
        mut target: TargetNode,
    ) -> NaviResult<()> {
        let nodes = vec![target.name.clone()];
        let job = parent.create_job(JobType::UploadKeys, nodes)?;
        job.run(|job| async move {
            if target.host.is_none() {
                return Err(NaviError::Unsupported);
            }

            let host = target.host.as_mut().unwrap();
            host.set_job(Some(job));
            host.upload_keys(&target.config.keys, true).await?;

            Ok(())
        })
        .await
    }

    /// Builds a system profile directly on the node itself.
    pub(crate) async fn build_on_node(
        self: &DeploymentHandle,
        parent: JobHandle,
        target: TargetNode,
        profile_drv: ProfileDerivation,
    ) -> NaviResult<(TargetNode, Profile)> {
        let builder = build::NodeBuilder::new(
            self.options.clone(),
            self.nix_options.clone(),
            self.goal,
            self.parallelism_limit.clone(),
            self.hive.context_dir().map(|p| p.to_path_buf()),
        );
        builder.build_on_node(parent, target, profile_drv).await
    }

    /// Builds and pushes a system profile on a node.
    pub(crate) async fn build_and_push_node(
        self: &DeploymentHandle,
        parent: JobHandle,
        target: TargetNode,
        profile_drv: ProfileDerivation,
    ) -> NaviResult<(TargetNode, Profile)> {
        let builder = build::NodeBuilder::new(
            self.options.clone(),
            self.nix_options.clone(),
            self.goal,
            self.parallelism_limit.clone(),
            self.hive.context_dir().map(|p| p.to_path_buf()),
        );
        builder
            .build_and_push_node(parent, target, profile_drv)
            .await
    }

    /// Activates a system profile on a node.
    ///
    /// This will also upload keys to the node.
    pub(crate) async fn activate_node(
        self: DeploymentHandle,
        parent: JobHandle,
        target: TargetNode,
        profile: Profile,
    ) -> NaviResult<()> {
        let activator = activate::NodeActivator::new(
            self.goal,
            self.options.clone(),
            self.parallelism_limit.clone(),
        );
        activator.activate_node(parent, target, profile).await
    }
}
