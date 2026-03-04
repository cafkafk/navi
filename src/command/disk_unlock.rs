use clap::Parser;

use crate::{
    error::{NaviError, NaviResult},
    nix::{Hive, NodeName},
};

/// Unlock a disk on a remote host (e.g. initrd with ZFS/LUKS)
#[derive(Parser, Debug)]
pub struct Opts {
    /// Host to unlock
    #[arg(value_name = "HOST")]
    target: String,
}

pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    let node_name = NodeName::new(opts.target.clone())?;

    tracing::info!("Retrieving deployment info for {}...", node_name.as_str());
    let config = hive
        .deployment_info_single(&node_name)
        .await?
        .ok_or_else(|| NaviError::Unknown {
            message: format!("Node '{}' not found in configuration", opts.target),
        })?;

    if !config.unlock.enable {
        return Err(NaviError::Unknown {
                message: format!("Disk unlocking is not enabled for node '{}'. Set 'deployment.unlock.enable = true;' in your configuration to use this command.", node_name.as_str())
            });
    }

    let mut host = config.to_ssh_host().ok_or_else(|| NaviError::Unknown {
        message: "Node does not have an SSH target configured".to_string(),
    })?;

    // Shared initrd configuration
    host.configure_for_initrd(&config.unlock);

    // Retrieve password if configured
    if let Some(ref cmd_str) = config.unlock.password_command {
        tracing::info!("Retrieving password using: {}", cmd_str);
    }

    tracing::info!(
        "Connecting to {} on port {}...",
        opts.target,
        config.unlock.port
    );

    host.unlock_disk(&config.unlock).await
}
