use std::path::PathBuf;

use crate::error::{NaviError, NaviResult};
use crate::job::{JobHandle, JobState, JobType};
use crate::nix::host::Local as LocalHost;
use crate::nix::NixFlags;
use crate::nix::{CopyDirection, CopyOptions, Goal, Profile, ProfileDerivation};

use super::{Options, ParallelismLimit, TargetNode};

pub struct NodeBuilder {
    pub options: Options,
    pub nix_options: NixFlags,
    pub goal: Goal,
    pub parallelism_limit: ParallelismLimit,
    pub context_dir: Option<PathBuf>,
}

impl NodeBuilder {
    pub fn new(
        options: Options,
        nix_options: NixFlags,
        goal: Goal,
        parallelism_limit: ParallelismLimit,
        context_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            options,
            nix_options,
            goal,
            parallelism_limit,
            context_dir,
        }
    }

    /// Builds a system profile directly on the node itself.
    pub async fn build_on_node(
        &self,
        parent: JobHandle,
        mut target: TargetNode,
        profile_drv: ProfileDerivation,
    ) -> NaviResult<(TargetNode, Profile)> {
        let nodes = vec![target.name.clone()];

        let permit = self.parallelism_limit.apply.acquire().await.unwrap();

        let build_job = parent.create_job(JobType::Build, nodes.clone())?;
        let (target, profile) = build_job
            .run(|job| async move {
                if target.host.is_none() {
                    return Err(NaviError::Unsupported);
                }

                let host = target.host.as_mut().unwrap();
                host.set_job(Some(job.clone()));

                host.copy_closure(
                    profile_drv.as_store_path(),
                    CopyDirection::ToRemote,
                    CopyOptions::default().include_outputs(true),
                )
                .await?;

                let profile = profile_drv.realize_remote(host).await?;

                job.success_with_message(format!("Built {:?} on target node", profile.as_path()))?;
                Ok((target, profile))
            })
            .await?;

        drop(permit);

        Ok((target, profile))
    }

    /// Builds and pushes a system profile on a node.
    pub async fn build_and_push_node(
        &self,
        parent: JobHandle,
        target: TargetNode,
        profile_drv: ProfileDerivation,
    ) -> NaviResult<(TargetNode, Profile)> {
        let nodes = vec![target.name.clone()];

        let permit = self.parallelism_limit.apply.acquire().await.unwrap();

        // Build system profile
        let build_job = parent.create_job(JobType::Build, nodes.clone())?;

        let self_nix_options = self.nix_options.clone();

        let profile: Profile = build_job
            .run(|job| async move {
                // FIXME: Remote builder?
                let mut builder = LocalHost::new(self_nix_options).upcast();
                builder.set_job(Some(job.clone()));

                let profile = profile_drv.realize(&mut builder).await?;

                job.success_with_message(format!("Built {:?}", profile.as_path()))?;
                Ok(profile)
            })
            .await?;

        // Create GC root
        let profile_r = profile.clone();
        let self_options = self.options.clone();
        let self_context_dir = self.context_dir.clone();

        let mut target = if self.options.create_gc_roots {
            let job = parent.create_job(JobType::CreateGcRoots, nodes.clone())?;
            job.run_waiting(|job| async move {
                if let Some(dir) = self_context_dir {
                    job.state(JobState::Running)?;
                    let path = dir.join(".gcroots").join(format!("node-{}", &*target.name));

                    profile_r.create_gc_root(&path).await?;
                } else {
                    job.noop("No context directory to create GC roots in".to_string())?;
                }
                Ok(target)
            })
            .await?
        } else {
            target
        };

        if self.goal == Goal::Build {
            return Ok((target, profile));
        }

        // Push closure to remote
        let push_job = parent.create_job(JobType::Push, nodes.clone())?;
        let push_profile = profile.clone();

        let target = push_job
            .run(|job| async move {
                if target.host.is_none() {
                    return Err(NaviError::Unsupported);
                }

                let host = target.host.as_mut().unwrap();
                host.set_job(Some(job.clone()));
                host.copy_closure(
                    push_profile.as_store_path(),
                    CopyDirection::ToRemote,
                    self_options.to_copy_options(),
                )
                .await?;

                Ok(target)
            })
            .await?;

        drop(permit);

        Ok((target, profile))
    }
}
