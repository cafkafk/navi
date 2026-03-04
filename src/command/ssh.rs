use clap::Parser;
use std::os::unix::process::CommandExt;

use crate::{
    error::{NaviError, NaviResult},
    nix::{Hive, NodeName, node_filter::NodeFilterOpts},
    util::confirm_action,
};

/// SSH into a host or manage host keys
#[derive(Parser, Debug)]
pub struct Opts {
    /// Host to connect to (required unless -R is used)
    #[arg(value_name = "HOST")]
    target: Option<String>,

    /// Command and arguments to pass to SSH
    #[arg(value_name = "COMMAND", last = true)]
    command: Vec<String>,

    /// Remove host keys from known_hosts
    #[arg(short = 'R', long)]
    remove_keys: bool,

    #[command(flatten)]
    node_filter: NodeFilterOpts,
}

pub async fn run(hive: Hive, opts: Opts) -> NaviResult<()> {
    if opts.remove_keys {
        // Resolve targets
        let targets: Vec<NodeName> = if let Some(t) = &opts.target {
            vec![NodeName::new(t.clone())?]
        } else {
            // Use filters. If no filter, select_nodes returns all.
            hive.select_nodes(opts.node_filter.on, None, false).await?
                .into_keys()
                .collect()
        };

        if targets.is_empty() {
            tracing::warn!("No nodes found to remove keys for.");
            return Ok(());
        }

        eprintln!("Found {} nodes to remove keys for.", targets.len());
        if targets.len() > 1 {
            if !confirm_action(&format!("Are you sure you want to remove host keys for {} nodes?", targets.len()))? {
                return Ok(());
            }
        }

        for node_name in targets {
            let config = hive.deployment_info_single(&node_name).await?;
            if let Some(config) = config {
                if let Some(host) = config.target_host {
                    // ssh-keygen -R hostname
                    tracing::info!("Removing key for {} ({})", node_name.as_str(), host);
                    
                    let status = std::process::Command::new("ssh-keygen")
                        .arg("-R")
                        .arg(&host)
                        .stdout(std::process::Stdio::null()) // Suppress stdout (it just says 'updated ..')
                        .status();

                    match status {
                        Ok(s) => {
                            if !s.success() {
                                tracing::warn!("ssh-keygen failed for {}", host);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to run ssh-keygen: {}", e);
                        }
                    }
                } else {
                    tracing::warn!("Node {} has no target_host configured, skipping.", node_name.as_str());
                }
            }
        }
        return Ok(());
    }

    // Interactive Mode
    let target_name = opts.target.ok_or_else(|| NaviError::Unknown { 
        message: "Target host required for interactive SSH (or use -R/--on for key removal)".to_string() 
    })?;

    let node_name = NodeName::new(target_name.clone())?;

    tracing::info!("Retrieving deployment info for {}...", node_name.as_str());
    let config = hive.deployment_info_single(&node_name).await?;

    if let Some(config) = config {
        let host = config.target_host.as_deref().unwrap_or("localhost");
        let user = config.target_user.as_deref().unwrap_or("root");
        let target = if host == "localhost" {
            "localhost".to_string()
        } else {
            format!("{}@{}", user, host)
        };

        let mut cmd = std::process::Command::new("ssh");

        // If no command is specified, force a TTY for interactive session
        let interactive = opts.command.is_empty();

        if interactive {
            cmd.arg("-t");
        }

        if let Some(ssh_host) = config.to_ssh_host() {
            let options = ssh_host.ssh_options();

            if interactive {
                // Filter out -T (disable pseudo-tty) since we want an interactive TTY
                let interactive_options = options
                    .into_iter()
                    .filter(|o| o != "-T")
                    .collect::<Vec<_>>();
                cmd.args(interactive_options);
            } else {
                cmd.args(options);
            }
        }

        cmd.arg(target);
        cmd.args(opts.command);

        tracing::info!("Exec: {:?}", cmd);

        let err = cmd.exec();

        // If we got here, exec failed
        return Err(NaviError::Unknown {
            message: format!("Failed to execute ssh: {}", err),
        });
    } else {
        return Err(NaviError::Unknown {
            message: format!("Node '{}' not found in configuration", target_name),
        });
    }
}
