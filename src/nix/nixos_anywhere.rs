use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use tokio::process::Command;
use tokio::time::sleep;

use crate::error::{NaviError, NaviResult};
use crate::nix::{
    deployment::TargetNode, hive::Hive, NixosAnywhereConfig, NodeName, Ssh,
};
use crate::util::{CommandExt, CommandExecution};

pub async fn run(
    hive: &Hive,
    targets: &HashMap<NodeName, TargetNode>,
    na_config: &NixosAnywhereConfig,
    tf_outputs: &serde_json::Value,
    unlock_after_install: bool,
    subset_nodes: Option<Vec<&str>>,
) -> NaviResult<()> {
    // Determine which nodes to process. If subset_nodes is provided, use it, otherwise use all targets.
    // However, we still filter by the provisioner in the caller usually, but here we just iterate
    // over the nodes we are given.
    let nodes_to_process: Vec<&str> = if let Some(subset) = subset_nodes {
        subset
    } else {
        targets.keys().map(|k| k.as_str()).collect()
    };

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for node in &nodes_to_process {
        // IP discovery convention: {node_name_with_underscores}_ip
        let ip_key = format!("{}_ip", node.replace("-", "_"));

        if let Some(ip_val) = tf_outputs.get(&ip_key) {
            if let Some(ip) = ip_val.get("value").and_then(|v| v.as_str()) {
                tracing::info!("Found IP for {}: {}", node, ip);

                // Find the target node configuration
                let target_node = targets
                    .iter()
                    .find(|(n, _)| n.as_str() == *node)
                    .map(|(_, t)| t);

                // Check if IAP is enabled and allow override of the target host
                let (ssh_host, _use_iap) = if let Some(target) = target_node {
                    match target.config.get_provider() {
                        crate::nix::host::Provider::Gcp { iap, .. } if iap => {
                            // If IAP is enabled, we MUST use the instance name (target_host or node name)
                            // because gcloud start-iap-tunnel requires the instance name, not IP.
                            let hostname = target
                                .config
                                .target_host
                                .clone()
                                .unwrap_or_else(|| node.to_string());
                            tracing::info!(
                                "IAP enabled for {}. Using instance name '{}' instead of IP.",
                                node,
                                hostname
                            );
                            (hostname, true)
                        }
                        _ => (ip.to_string(), false),
                    }
                } else {
                    (ip.to_string(), false)
                };

                // Create a temporary Ssh helper to generate the correct options (including ProxyCommand)
                let ssh_helper = if let Some(target) = target_node {
                    // Use config from the node if available (preserves User, Port, etc.)

                    let mut h = target.config.to_ssh_host().unwrap_or_else(|| {
                        Ssh::new(Some("root".to_string()), ssh_host.clone())
                    });
                    // Force the host to be what we determined above (IP or Instance Name)
                    h.set_override_address(ssh_host.clone());
                    // Ensure provider is set so ssh_options() generates IAP config
                    h.set_provider(target.config.get_provider());
                    h
                } else {
                    Ssh::new(Some("root".to_string()), ssh_host.clone())
                };

                // Add strict checking options for security, unless disabled
                let ssh_opts = ssh_helper.ssh_options();

                // Determine user to connect as
                let node_target_user = targets
                    .iter()
                    .find(|(n, _)| n.as_str() == *node)
                    .and_then(|(_, t)| t.config.target_user.as_deref());

                let ssh_user = na_config
                    .ssh_user
                    .as_deref()
                    .or(node_target_user)
                    .unwrap_or("root");

                let target_str = format!("{}@{}", ssh_user, ssh_host);
                
                // Wait for connectivity before launching nixos-anywhere
                // This is crucial for IAP tunnels which take time to establish/propagate
                if let Err(e) = wait_for_connectivity(&target_str, &ssh_opts).await {
                    tracing::error!("Skipping node {} due to connectivity/auth issues: {}", node, e);
                    failed.push((node.to_string(), e.to_string()));
                    continue;
                }

                let flake_ref = format!(".#{}", node);

                tracing::info!(
                    "Executing: nixos-anywhere --flake {} {}",
                    flake_ref,
                    target_str
                );

                let mut na_cmd = Command::new("nixos-anywhere");
                na_cmd.arg("--flake").arg(&flake_ref).arg(&target_str);

                if na_config.download_kexec_locally {
                    match get_node_system(hive, node).await {
                        Ok(system) => {
                            let template = na_config
                                .kexec_url_template
                                .as_deref()
                                .unwrap_or(crate::nix::DEFAULT_KEXEC_URL_TEMPLATE);
                            let url = get_kexec_url(template, &system);
                            let kexec_path = PathBuf::from(format!("/tmp/nixos-kexec-{}-{}.tar.gz", node, system));

                            let download_res = if !kexec_path.exists() {
                                tracing::info!("Downloading kexec image for {} ({}) from {}...", node, system, url);
                                download_file(&url, &kexec_path).await
                            } else {
                                tracing::info!("Using cached kexec image at {:?}", kexec_path);
                                Ok(())
                            };

                            if let Err(e) = download_res {
                                tracing::error!("Failed to setup kexec for {}: {}", node, e);
                                failed.push((node.to_string(), e.to_string()));
                                continue;
                            }

                            na_cmd.arg("--kexec").arg(kexec_path);
                        },
                        Err(e) => {
                            tracing::error!("Failed to get node system for {}: {}", node, e);
                            failed.push((node.to_string(), e.to_string()));
                            continue;
                        }
                    }
                }

                // Add extra_args from config to the command
                for arg in &na_config.extra_args {
                    na_cmd.arg(arg);
                }

                populate_ssh_args(&mut na_cmd, &ssh_opts);

                match na_cmd.passthrough().await {
                    Ok(_) => {
                        if unlock_after_install || na_config.unlock {
                            if let Err(e) = unlock_node(hive, node).await {
                                tracing::warn!("Installation succeeded but unlock failed for {}: {}", node, e);
                                // We consider install successful even if unlock failed partially?
                                // User might want to know.
                                succeeded.push(node.to_string());
                            } else {
                                succeeded.push(node.to_string());
                            }
                        } else {
                            succeeded.push(node.to_string());
                        }
                    }
                    Err(e) => {
                        tracing::error!("nixos-anywhere failed for {}: {}", node, e);
                        failed.push((node.to_string(), e.to_string()));
                        continue;
                    }
                }
            } else {
                tracing::warn!("Output {} found but has no string value", ip_key);
                failed.push((node.to_string(), "Missing IP output".into()));
            }
        } else {
            tracing::warn!(
                "No IP output found for node {} (expected output: {}). Skipping nixos-anywhere.",
                node,
                ip_key
            );
            failed.push((node.to_string(), "Missing IP output".into()));
        }
    }

    // Print summary
    if !nodes_to_process.is_empty() {
        println!("\n--- Installation Summary ---");
        if !succeeded.is_empty() {
            println!("Succeeded:");
            for n in &succeeded {
                println!("  ✅ {}", n);
            }
        }
        if !failed.is_empty() {
            println!("Failed/Skipped:");
            for (n, e) in &failed {
                println!("  ❌ {} (Error: {})", n, e);
            }
        }
        println!("----------------------------");
    }

    Ok(())
}

fn populate_ssh_args(cmd: &mut Command, ssh_opts: &[String]) {
    let mut iter = ssh_opts.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" => {
                if let Some(val) = iter.next() {
                    cmd.arg("--ssh-option").arg(val);
                }
            }
            "-p" => {
                if let Some(val) = iter.next() {
                    cmd.arg("--ssh-port").arg(val);
                }
            }
            "-i" => {
                if let Some(val) = iter.next() {
                    cmd.arg("-i").arg(val);
                }
            }
            "-F" => {
                if let Some(val) = iter.next() {
                    // Explicitly set the config file environment variable for underlying ssh/scp
                    cmd.env("SSH_CONFIG_FILE", val);

                    // Parse parsing logic to workaround IAP/ProxyCommand quoting issues in nixos-anywhere
                    let path = std::path::Path::new(val);
                    if path.exists() {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            for line in content.lines() {
                                let line = line.trim();
                                if let Some(c) = line.strip_prefix("ProxyCommand ") {
                                    // Create a wrapper script to avoid quoting issues
                                    let wrapper_path = path.with_extension("proxy.sh");
                                    
                                    // Replace %h and %p with shell arguments $1 and $2
                                    let script_cmd = c.replace("%h", "$1").replace("%p", "$2");
                                    
                                    let wrapper_content = format!("#!/bin/sh\n{}\n", script_cmd);
                                    if std::fs::write(&wrapper_path, wrapper_content).is_ok() {
                                        let mut perms = std::fs::metadata(&wrapper_path).unwrap().permissions();
                                        perms.set_mode(0o755);
                                        std::fs::set_permissions(&wrapper_path, perms).ok();
                                        
                                        // Tell SSH to run the wrapper script with %h and %p as arguments
                                        cmd.arg("--ssh-option").arg(format!("ProxyCommand='{}' %h %p", wrapper_path.display()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "-T" => {
                // nixos-anywhere handles TTY allocation itself primarily, and -T might interfere or be unnecessary
            }
            x => {
                tracing::debug!("Ignored SSH arg for nixos-anywhere: {}", x);
            }
        }
    }
}

async fn wait_for_connectivity(target: &str, ssh_opts: &[String]) -> NaviResult<()> {
    tracing::info!("Waiting for connectivity to {}...", target);
    
    // Increase timeout to 10 minutes (600s) to accommodate slow IAP tunnel propagation
    // and instance boot times, especially during reinstall/provisioning.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    let mut permission_denied_count = 0;
    let mut last_error_msg = String::new();

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏✅")
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap(),
    );
    pb.set_message(format!("Connecting to {}...", target));
    pb.enable_steady_tick(Duration::from_millis(100));

    loop {
        if tokio::time::Instant::now() > deadline {
            pb.finish_and_clear();
            return Err(NaviError::DeploymentError {
                message: format!("Timed out waiting for connectivity to {}", target),
            });
        }

        let mut cmd = Command::new("ssh");
        
        // Pass standard SSH opts directly.
        cmd.args(ssh_opts);
        
        // Add robust options for checking
        // We use exit 0 so it's a no-op on the server side
        // Added UserKnownHostsFile=/dev/null to avoid host key mismatch errors during reinstall
        cmd.args([
            "-o", "ConnectTimeout=5",
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null", 
            target, "exit", "0"
        ]);

        // We capture output to check for specific IAP errors
        let mut exec = CommandExecution::new(cmd);
        exec.set_quiet(true);
        
        match exec.run().await {
            Ok(_) => {
                pb.finish_with_message("Connection established successfully!");
                return Ok(());
            }
            Err(_) => {
                let (_, stderr) = exec.get_logs();
                if let Some(err) = stderr {
                    if err.contains("Permission denied") {
                        permission_denied_count += 1;
                        pb.set_message(format!("Permission denied (attempt {}/5)", permission_denied_count));
                        if permission_denied_count >= 5 {
                            pb.finish_and_clear();
                            return Err(NaviError::DeploymentError {
                                message: format!("Host {} appears softlocked (Permission denied > 5 times). Skipping.", target),
                            });
                        }
                    } else if err.contains("Failed to lookup instance") {
                        pb.set_message("GCP: Failed to lookup instance (waiting for API propagation)...");
                    } else if err.contains("failed to connect to backend") {
                        pb.set_message("GCP: Failed to connect to backend (waiting for IAP)...");
                    } else {
                        // Clean up the error message for display
                        let clean_err = err.replace("ERROR: ", "").trim().to_string();
                        let first_line = clean_err.lines().next().unwrap_or("Unknown error").trim();
                        
                        // If this is a new/different error, log it to the console above the spinner
                        // This helps debugging without cluttering the spinner status
                        if first_line != last_error_msg && !first_line.is_empty() {
                            // Filter out known noise lines like the ssh banner
                            if !first_line.starts_with("Warning: Permanently added") {
                                pb.println(format!("  [Debug] SSH Error: {}", first_line));
                                last_error_msg = first_line.to_string();
                            }
                        }
                        
                        // Update spinner with truncated version
                        let short_err = first_line.chars().take(60).collect::<String>();
                        pb.set_message(format!("Retrying: {}...", short_err));
                    }
                }
            }
        }

        sleep(Duration::from_secs(5)).await;
    }
}

async fn unlock_node(hive: &Hive, node: &str) -> NaviResult<()> {
    tracing::info!("Retrieving deployment info for {}...", node);
    let node_name = NodeName::new(node.to_string())?;

    // Retrieve sensitive config (secrets/keys) which requires evaluation
    let info = hive.deployment_info_single(&node_name).await?;

    if let Some(config) = info {
        if config.unlock.enable {
            if let Some(mut host) = config.to_ssh_host() {
                host.configure_for_initrd(&config.unlock);

                // Set aggressive timeouts for polling
                let mut retry_opts = config.unlock.ssh_options.clone();
                retry_opts.extend([
                    "-o".to_string(),
                    "ConnectTimeout=5".to_string(),
                    "-o".to_string(),
                    "ConnectionAttempts=1".to_string(),
                ]);
                host.set_extra_ssh_options(retry_opts);

                tracing::info!("Waiting for node {} to be ready for unlock...", node);

                let mut unlocked = false;
                // Try for ~5 minutes (60 * 5s)
                for i in 1..=60 {
                    match host.unlock_disk(&config.unlock).await {
                        Ok(_) => {
                            tracing::info!("Disk unlocked successfully for {}!", node);
                            unlocked = true;
                            break;
                        }
                        Err(e) => {
                            tracing::debug!("Unlock attempt {} failed: {}", i, e);
                            sleep(Duration::from_secs(5)).await;
                        }
                    }
                }

                if !unlocked {
                    tracing::error!(
                        "Failed to unlock disk for {} after multiple attempts.",
                        node
                    );
                }
            } else {
                tracing::error!("Could not create SSH host for {}", node);
            }
        } else {
            tracing::info!("Unlock not enabled for {}, skipping.", node);
        }
    } else {
        tracing::error!("Could not find deployment info for {}", node);
    }
    Ok(())
}

async fn get_node_system(hive: &Hive, node: &str) -> NaviResult<String> {
    let expr = format!("x: x.nodes.\"{}\".pkgs.system", node);
    let out = hive.introspect(expr, false).await?;
    let s: String = serde_json::from_str(&out).map_err(|_| NaviError::DeploymentError { 
        message: format!("Failed to parse system for node {}: {}", node, out) 
    })?;
    Ok(s)
}

fn get_kexec_url(template: &str, system: &str) -> String {
    template.replace("{}", system)
}

async fn download_file(url: &str, path: &Path) -> NaviResult<()> {
    let mut cmd = Command::new("curl");
    cmd.arg("-L").arg("-o").arg(path).arg(url);
    let status = cmd.status().await.map_err(|e| NaviError::IoError { error: e })?;

    if !status.success() {
        return Err(NaviError::DeploymentError { 
            message: format!("Failed to download kexec image from {}", url) 
        });
    }
    Ok(())
}
