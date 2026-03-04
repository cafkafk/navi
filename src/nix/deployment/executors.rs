use std::collections::HashMap;

use async_trait::async_trait;
use futures::future::join_all;
use itertools::Itertools;
use tokio_stream::StreamExt;

use crate::error::{NaviError, NaviResult};
use crate::job::{JobHandle, JobState, JobType};
use crate::nix::deployment::{DeploymentHandle, TargetNodeMap};
use crate::nix::evaluator::{DrvSetEvaluator, EvalError, NixEvalJobs};
use crate::nix::{NodeName, ProfileDerivation};

#[async_trait]
pub trait DeploymentExecutor: Send + Sync {
    async fn execute(
        &self,
        deployment: &DeploymentHandle,
        parent: JobHandle,
        targets: TargetNodeMap,
    ) -> NaviResult<()>;
}

pub struct ChunkedExecutor;

#[async_trait]
impl DeploymentExecutor for ChunkedExecutor {
    async fn execute(
        &self,
        deployment: &DeploymentHandle,
        parent: JobHandle,
        mut targets: TargetNodeMap,
    ) -> NaviResult<()> {
        let eval_limit = deployment
            .evaluation_node_limit
            .get_limit()
            .unwrap_or(targets.len());

        let mut futures = Vec::new();

        for chunk in targets.drain().chunks(eval_limit).into_iter() {
            let mut map = HashMap::new();
            for (name, host) in chunk {
                map.insert(name, host);
            }

            futures.push(execute_one_chunk(deployment, parent.clone(), map));
        }

        join_all(futures)
            .await
            .into_iter()
            .collect::<NaviResult<Vec<()>>>()?;

        Ok(())
    }
}

async fn execute_one_chunk(
    deployment: &DeploymentHandle,
    parent: JobHandle,
    mut chunk: TargetNodeMap,
) -> NaviResult<()> {
    if deployment.goal == crate::nix::Goal::UploadKeys {
        unreachable!(); // some logic is screwed up
    }

    let nodes: Vec<NodeName> = chunk.keys().cloned().collect();
    let profile_drvs = deployment
        .evaluate_nodes(parent.clone(), nodes.clone())
        .await?;

    let mut futures = Vec::new();

    for (name, profile_drv) in profile_drvs.iter() {
        let mut target = chunk.remove(name).unwrap();

        if let Some(force_build_on_target) = deployment.options.force_build_on_target {
            target.config.set_build_on_target(force_build_on_target);
        }

        let job_handle = parent.clone();
        let arc_self = deployment.clone();
        futures.push(async move {
            let (target, profile) = {
                if target.config.build_on_target() {
                    arc_self
                        .build_on_node(job_handle.clone(), target, profile_drv.clone())
                        .await?
                } else {
                    arc_self
                        .build_and_push_node(job_handle.clone(), target, profile_drv.clone())
                        .await?
                }
            };

            if arc_self.goal.requires_activation() {
                arc_self.activate_node(job_handle, target, profile).await
            } else {
                Ok(())
            }
        });
    }

    join_all(futures)
        .await
        .into_iter()
        .collect::<NaviResult<Vec<()>>>()?;

    Ok(())
}

pub struct StreamingExecutor;

#[async_trait]
impl DeploymentExecutor for StreamingExecutor {
    async fn execute(
        &self,
        deployment: &DeploymentHandle,
        parent: JobHandle,
        mut targets: TargetNodeMap,
    ) -> NaviResult<()> {
        if deployment.goal == crate::nix::Goal::UploadKeys {
            unreachable!(); // some logic is screwed up
        }

        let nodes: Vec<NodeName> = targets.keys().cloned().collect();
        let expr = deployment.hive.eval_selected_expr(&nodes)?;

        let job = parent.create_job(JobType::Evaluate, nodes.clone())?;

        let (futures, failed_attributes) = job
            .run(|job| async move {
                let mut evaluator = NixEvalJobs::default();
                let eval_limit = deployment
                    .evaluation_node_limit
                    .get_limit()
                    .unwrap_or(deployment.targets.len());
                evaluator.set_eval_limit(eval_limit);
                evaluator.set_job(job.clone());

                // FIXME: nix-eval-jobs currently does not support IFD with builders
                let options = deployment.hive.nix_flags();
                let mut stream = evaluator.evaluate(&expr, options).await?;

                let mut futures: Vec<tokio::task::JoinHandle<NaviResult<()>>> = Vec::new();
                let mut failed_attributes = Vec::new();

                while let Some(item) = stream.next().await {
                    match item {
                        Ok(attr) => {
                            let node_name = NodeName::new(attr.attribute().to_owned())?;
                            let profile_drv: ProfileDerivation = attr.into_derivation()?;

                            // FIXME: Consolidate
                            let mut target = targets.remove(&node_name).unwrap();

                            if let Some(force_build_on_target) =
                                deployment.options.force_build_on_target
                            {
                                target.config.set_build_on_target(force_build_on_target);
                            }

                            let job_handle = job.clone();
                            let arc_self = deployment.clone();
                            futures.push(tokio::spawn(async move {
                                let (target, profile) = {
                                    if target.config.build_on_target() {
                                        arc_self
                                            .build_on_node(
                                                job_handle.clone(),
                                                target,
                                                profile_drv.clone(),
                                            )
                                            .await?
                                    } else {
                                        arc_self
                                            .build_and_push_node(
                                                job_handle.clone(),
                                                target,
                                                profile_drv.clone(),
                                            )
                                            .await?
                                    }
                                };

                                if arc_self.goal.requires_activation() {
                                    arc_self.activate_node(job_handle, target, profile).await
                                } else {
                                    Ok(())
                                }
                            }));
                        }
                        Err(e) => {
                            match e {
                                EvalError::Global(e) => {
                                    // Global error - Abort immediately
                                    return Err(e);
                                }
                                EvalError::Attribute(e) => {
                                    // Attribute-level error
                                    //
                                    // NOTE: We still let the rest of the evaluation finish but
                                    // mark the whole Evaluate job as failed.

                                    let node_name =
                                        NodeName::new(e.attribute().to_string()).unwrap();
                                    let nodes = vec![node_name.clone()];
                                    let job = parent.create_job(JobType::Evaluate, nodes)?;

                                    job.state(JobState::Running)?;
                                    for line in e.error().lines() {
                                        job.stderr(line.to_string())?;
                                    }
                                    job.state(JobState::Failed)?;

                                    failed_attributes.push(node_name);
                                }
                            }
                        }
                    }
                }

                // HACK: Still return Ok() because we need to wait for existing jobs to finish
                if !failed_attributes.is_empty() {
                    job.failure(&NaviError::AttributeEvaluationError)?;
                }

                Ok((futures, failed_attributes))
            })
            .await?;

        join_all(futures)
            .await
            .into_iter()
            .map(|r| r.unwrap()) // panic on JoinError (future panicked)
            .collect::<NaviResult<Vec<()>>>()?;

        if !failed_attributes.is_empty() {
            Err(NaviError::AttributeEvaluationError)
        } else {
            Ok(())
        }
    }
}
