use std::collections::HashMap;

use crate::error::{NaviError, NaviResult};
use crate::job::{JobHandle, JobState, JobType};
use crate::nix::{
    key::{Key, UploadAt as UploadKeyAt},
    Goal, Profile, Provenance, RebootOptions,
};

use super::{Options, ParallelismLimit, TargetNode};

pub struct NodeActivator {
    pub goal: Goal,
    pub options: Options,
    pub parallelism_limit: ParallelismLimit,
}

impl NodeActivator {
    pub fn new(goal: Goal, options: Options, parallelism_limit: ParallelismLimit) -> Self {
        Self {
            goal,
            options,
            parallelism_limit,
        }
    }

    /// Activates a system profile on a node.
    ///
    /// This will also upload keys to the node.
    pub async fn activate_node(
        &self,
        parent: JobHandle,
        mut target: TargetNode,
        profile: Profile,
    ) -> NaviResult<()> {
        let nodes = vec![target.name.clone()];

        let permit = self.parallelism_limit.apply.acquire().await.unwrap();

        // Upload pre-activation keys
        let mut target = if self.options.upload_keys {
            let job = parent.create_job(JobType::UploadKeys, nodes.clone())?;
            job.run_waiting(|job| async move {
                let keys = target
                    .config
                    .keys
                    .iter()
                    .filter(|(_, v)| v.upload_at() == UploadKeyAt::PreActivation)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<String, Key>>();

                if keys.is_empty() {
                    job.noop("No pre-activation keys to upload".to_string())?;
                    return Ok(target);
                }

                job.state(JobState::Running)?;
                job.message("Uploading pre-activation keys...".to_string())?;

                let host = target.host.as_mut().unwrap();
                host.set_job(Some(job.clone()));
                host.upload_keys(&keys, false).await?;

                job.success_with_message("Uploaded keys (pre-activation)".to_string())?;
                Ok(target)
            })
            .await?
        } else {
            target
        };

        // Activate profile
        let activation_job = parent.create_job(JobType::Activate, nodes.clone())?;
        let self_goal = self.goal;
        let self_options = self.options.clone();
        let profile_r = profile.clone();
        let mut target = activation_job.run(|job| async move {
            let host = target.host.as_mut().unwrap();
            host.set_job(Some(job.clone()));

            if !target.config.replace_unknown_profiles {
                job.message("Checking remote profile...".to_string())?;

                let profile = host.get_main_system_profile().await?;

                if profile.as_store_path().exists() {
                    job.message("Remote profile known".to_string())?;
                } else if self_options.force_replace_unknown_profiles {
                    job.message("Warning: Remote profile is unknown, but unknown profiles are being ignored".to_string())?;
                } else {
                    return Err(NaviError::ActiveProfileUnknown {
                        profile,
                    });
                }
            }

            host.activate(&profile_r, self_goal, self_options.install_bootloader).await?;

            if matches!(self_goal, Goal::Switch | Goal::Boot) {
                job.message("Writing provenance metadata...".to_string())?;
                
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string();
                
                let deployed_by = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
                
                let commit = {
                    let output = std::process::Command::new("git")
                        .args(["rev-parse", "HEAD"])
                        .output();
                    match output {
                        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
                        Err(_) => "unknown".to_string(),
                    }
                };
                
                let flake_uri = "unknown".to_string(); // TODO: Extract from hive path properly

                let provenance = Provenance {
                    timestamp,
                    deployed_by,
                    commit,
                    flake_uri,
                };
                
                host.write_provenance(&provenance).await?;
            }

            job.success_with_message(self_goal.success_str().to_string())?;

            Ok(target)
        }).await?;

        // Reboot
        let mut target = if self.options.reboot {
            let job = parent.create_job(JobType::Reboot, nodes.clone())?;
            let self_goal_reboot = self.goal;
            job.run(|job| async move {
                let host = target.host.as_mut().unwrap();
                host.set_job(Some(job.clone()));

                let new_profile = if self_goal_reboot.persists_after_reboot() {
                    Some(profile)
                } else {
                    None
                };

                let mut options = RebootOptions::default().wait_for_boot(true);

                if let Some(profile) = new_profile {
                    options.new_profile = Some(profile);
                }

                if target.config.unlock.enable {
                    options = options.unlock(Some(target.config.unlock.clone()));
                }

                host.reboot(options).await?;

                Ok(target)
            })
            .await?
        } else {
            target
        };

        // Upload post-activation keys
        if self.options.upload_keys {
            let job = parent.create_job(JobType::UploadKeys, nodes.clone())?;
            job.run_waiting(|job| async move {
                let keys = target
                    .config
                    .keys
                    .iter()
                    .filter(|(_, v)| v.upload_at() == UploadKeyAt::PostActivation)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<String, Key>>();

                if keys.is_empty() {
                    job.noop("No post-activation keys to upload".to_string())?;
                    return Ok(());
                }

                job.state(JobState::Running)?;
                job.message("Uploading post-activation keys...".to_string())?;

                let host = target.host.as_mut().unwrap();
                host.set_job(Some(job.clone()));
                host.upload_keys(&keys, true).await?;

                job.success_with_message("Uploaded keys (post-activation)".to_string())?;
                Ok(())
            })
            .await?;
        }

        drop(permit);

        Ok(())
    }
}
