use clap::Args;
use crate::error::{NaviError, NaviResult};
use crate::nix::{hive::Hive, NodeName, Host};

#[derive(Debug, Args)]
#[command(name = "serial", about = "Connect to the serial console of a node")]
pub struct Opts {
    /// The node to connect to
    pub node: String,
}

pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    let node_name = NodeName::new(opts.node.clone())?;
    
    // We need deployment info to construct the Host (Ssh) with correct provider details
    let info = hive.deployment_info_single(&node_name).await?.ok_or_else(|| {
        NaviError::DeploymentError {
            message: format!("Node '{}' not found or has no deployment info", opts.node),
        }
    })?;

    // Create the host (usually Ssh)
    if let Some(mut host) = info.to_ssh_host() {
        host.connect_serial().await
    } else {
        Err(NaviError::Unsupported)
    }
}
