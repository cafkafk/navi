use tokio::sync::mpsc::{self, UnboundedSender};

use super::events::AppEvent;
use super::logging::TuiOutput;
use super::model::DeploySettings; // Add import
use crate::nix::hive::HivePath;
use crate::nix::host::{Host, Local as LocalHost};
use crate::nix::{CopyDirection, CopyOptions, Goal, Hive, NixFlags, NodeName, StorePath};
use crate::progress::Message;
use crate::progress::ProgressOutput;
use crate::progress::{Line, LineStyle};
use uuid::Uuid;

pub fn start_deployment(
    hive_path: HivePath,
    selected_nodes: Vec<NodeName>,
    parallel: usize,
    tx: UnboundedSender<AppEvent>,
    settings: DeploySettings, // Add arg
) {
    let task_id = Uuid::new_v4();
    let _ = tx.send(AppEvent::TaskStarted(
        task_id,
        format!("Deployment ({} nodes)", selected_nodes.len()),
    ));

    tokio::spawn(async move {
        match Hive::new(hive_path).await {
            Ok(hive) => {
                let targets_result = hive.select_nodes(None, None, false).await;
                match targets_result {
                    Ok(mut targets) => {
                        targets.retain(|name, _| selected_nodes.contains(name));
                        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                            Line::new(
                                crate::job::JobId::new(),
                                format!(
                                    "Deployment will proceed for {} filtered node(s).",
                                    targets.len()
                                ),
                            )
                            .style(LineStyle::Normal)
                            .label("System".to_string()),
                        )));

                        if targets.is_empty() {
                            let _ = tx.send(AppEvent::TaskFinished(task_id));
                            return;
                        }

                        // Bridge for TuiOutput since it expects UnboundedSender<Message>
                        let (d_tx, mut d_rx) = mpsc::unbounded_channel::<Message>();
                        let proxy_tx = tx.clone();
                        tokio::spawn(async move {
                            while let Some(msg) = d_rx.recv().await {
                                let _ = proxy_tx.send(AppEvent::Progress(msg));
                            }
                        });

                        let mut output = TuiOutput { sender: d_tx };
                        let progress = output.get_sender();
                        let mut deployment = crate::nix::deployment::Deployment::new(
                            hive,
                            targets,
                            Goal::Switch,
                            progress,
                        );
                        let mut limit = crate::nix::deployment::ParallelismLimit::default();
                        limit.set_apply_limit(parallel);
                        deployment.set_parallelism_limit(limit);

                        // Apply settings
                        let mut options = crate::nix::deployment::Options::default();
                        options.set_reboot(settings.reboot);
                        options.set_install_bootloader(settings.install_bootloader);
                        options.set_upload_keys(!settings.no_keys);
                        options.set_substituters_push(!settings.no_substitute);
                        options.set_gzip(!settings.no_gzip);
                        options.set_force_replace_unknown_profiles(
                            settings.force_replace_unknown_profiles,
                        );
                        if settings.keep_result {
                            options.set_create_gc_roots(true);
                        }
                        if settings.build_on_target {
                            options.set_force_build_on_target(true);
                        }

                        deployment.set_options(options);

                        if let Err(e) = deployment.execute().await {
                            let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                                Line::new(
                                    crate::job::JobId::new(),
                                    format!("Deployment error: {}", e),
                                )
                                .style(LineStyle::Failure)
                                .label("System".to_string()),
                            )));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                            Line::new(crate::job::JobId::new(), format!("Selection error: {}", e))
                                .style(LineStyle::Failure)
                                .label("System".to_string()),
                        )));
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                    Line::new(crate::job::JobId::new(), format!("Hive error: {}", e))
                        .style(LineStyle::Failure)
                        .label("System".to_string()),
                )));
            }
        }
        let _ = tx.send(AppEvent::TaskFinished(task_id));
    });
}

pub fn start_garbage_collection(
    hive_path: HivePath,
    selected_nodes: Vec<NodeName>,
    interval: Option<String>,
    tx: UnboundedSender<AppEvent>,
) {
    let task_id = Uuid::new_v4();
    let _ = tx.send(AppEvent::TaskStarted(
        task_id,
        format!("GC ({} nodes)", selected_nodes.len()),
    ));

    tokio::spawn(async move {
        // Log start
        let interval_str = interval.clone().unwrap_or("all old".to_string());
        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
            Line::new(
                crate::job::JobId::new(),
                format!("Starting Garbage Collection ({})...", interval_str),
            )
            .style(LineStyle::Normal)
            .label("GC".to_string()),
        )));

        match Hive::new(hive_path).await {
            Ok(hive) => {
                let targets_result = hive.select_nodes(None, None, false).await;
                match targets_result {
                    Ok(mut targets) => {
                        targets.retain(|name, _| selected_nodes.contains(name));
                        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                            Line::new(
                                crate::job::JobId::new(),
                                format!("GC will proceed for {} filtered node(s).", targets.len()),
                            )
                            .style(LineStyle::Normal)
                            .label("GC".to_string()),
                        )));

                        // Run in parallel (naive)
                        let mut handles = Vec::new();

                        for (name, target) in targets {
                            let tx_inner = tx.clone();
                            let interval_inner = interval.clone();
                            let target_name = name.clone();
                            let config = target.config.clone();

                            handles.push(tokio::spawn(async move {
                                let _host_str =
                                    config.target_host.as_deref().unwrap_or("localhost");

                                let _ = tx_inner.send(AppEvent::Progress(Message::Print(
                                    Line::new(
                                        crate::job::JobId::new(),
                                        "Connecting...".to_string(),
                                    )
                                    .style(LineStyle::Normal)
                                    .label(target_name.as_str().to_string()),
                                )));

                                // Create host instance
                                let mut host: Box<dyn Host> =
                                    if let Some(ssh) = config.to_ssh_host() {
                                        ssh.upcast()
                                    } else {
                                        LocalHost::new(NixFlags::default()).upcast()
                                    };

                                let mut cmd_args = vec!["nix-collect-garbage"];
                                let interval_arg; // needs to live long enough

                                if let Some(ref i) = interval_inner {
                                    cmd_args.push("--delete-older-than");
                                    interval_arg = i.clone();
                                    cmd_args.push(&interval_arg);
                                } else {
                                    cmd_args.push("-d");
                                }

                                match host.run_command(&cmd_args).await {
                                    Ok(_) => {
                                        let _ = tx_inner.send(AppEvent::Progress(Message::Print(
                                            Line::new(
                                                crate::job::JobId::new(),
                                                "Garbage collection completed.".to_string(),
                                            )
                                            .style(LineStyle::Success)
                                            .label(target_name.as_str().to_string()),
                                        )));
                                    }
                                    Err(e) => {
                                        let _ = tx_inner.send(AppEvent::Progress(Message::Print(
                                            Line::new(
                                                crate::job::JobId::new(),
                                                format!("GC Failed: {}", e),
                                            )
                                            .style(LineStyle::Failure)
                                            .label(target_name.as_str().to_string()),
                                        )));
                                    }
                                }
                            }));
                        }

                        // Wait for all
                        for h in handles {
                            let _ = h.await;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                            Line::new(crate::job::JobId::new(), format!("Selection error: {}", e))
                                .style(LineStyle::Failure)
                                .label("GC".to_string()),
                        )));
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                    Line::new(crate::job::JobId::new(), format!("Hive error: {}", e))
                        .style(LineStyle::Failure)
                        .label("GC".to_string()),
                )));
            }
        }

        let _ = tx.send(AppEvent::TaskFinished(task_id)); // Stop spinner/deploy mode
    });
}

pub fn start_diff(hive_path: HivePath, node_name: NodeName, tx: UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
            Line::new(
                crate::job::JobId::new(),
                format!("Starting Diff for {}...", node_name.as_str()),
            )
            .style(LineStyle::Normal)
            .label("Diff".to_string()),
        )));

        // 1. Get Node Config (to find target host)
        let hive = match Hive::new(hive_path.clone()).await {
            Ok(h) => h,
            Err(e) => {
                let _ = tx.send(AppEvent::DiffComputed(format!(
                    "Failed to load Hive: {}",
                    e
                )));
                return;
            }
        };

        // Use deployment_info_single to specific node config
        let config = match hive.deployment_info_single(&node_name).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                let _ = tx.send(AppEvent::DiffComputed("Node not found.".to_string()));
                return;
            }
            Err(e) => {
                let _ = tx.send(AppEvent::DiffComputed(format!(
                    "Failed to retrieve config: {}",
                    e
                )));
                return;
            }
        };

        // 2. Fetch Remote System Path
        let host_str = config.target_host.as_deref().unwrap_or("localhost");
        let user = config.target_user.as_deref().unwrap_or("root");
        let target = if host_str == "localhost" {
            "localhost".to_string()
        } else {
            format!("{}@{}", user, host_str)
        };

        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
            Line::new(
                crate::job::JobId::new(),
                format!("Fetching remote state from {}...", target),
            )
            .style(LineStyle::Normal)
            .label("Diff".to_string()),
        )));

        let _remote_path_res = if host_str == "localhost" {
            // Local readlink
            tokio::fs::read_link("/run/current-system")
                .await
                .map(|p| p.to_string_lossy().to_string())
        } else {
            // SSH
            let mut host: Box<dyn Host> = if let Some(ssh) = config.to_ssh_host() {
                ssh.upcast()
            } else {
                LocalHost::new(NixFlags::default()).upcast()
            };

            match host.get_main_system_profile().await {
                Ok(p) => Ok(p.as_path().to_str().unwrap().to_string()),
                Err(e) => Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{}", e),
                )),
            }
        };

        let remote_path_string = if host_str == "localhost" {
            tokio::fs::read_link("/run/current-system")
                .await
                .map(|p| p.to_string_lossy().to_string())
        } else {
            let output = tokio::process::Command::new("ssh")
                .arg(&target)
                .arg("readlink -f /run/current-system")
                .output()
                .await;

            match output {
                Ok(o) => {
                    if o.status.success() {
                        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            String::from_utf8_lossy(&o.stderr),
                        ))
                    }
                }
                Err(e) => Err(e),
            }
        };

        let remote_path = match remote_path_string {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(AppEvent::DiffComputed(format!(
                    "Failed to fetch remote system path: {}",
                    e
                )));
                return;
            }
        };

        if host_str != "localhost" {
            let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                Line::new(
                    crate::job::JobId::new(),
                    "Copying remote closure info to local store...".to_string(),
                )
                .style(LineStyle::Normal)
                .label("Diff".to_string()),
            )));

            if let Some(ssh) = config.to_ssh_host() {
                let mut host = ssh.upcast();
                // Create a store path from the string
                if let Ok(store_path) = StorePath::try_from(remote_path.clone()) {
                    let copy_opts = CopyOptions::default().include_outputs(true);
                    if let Err(e) = host
                        .copy_closure(&store_path, CopyDirection::FromRemote, copy_opts)
                        .await
                    {
                        let _ = tx.send(AppEvent::DiffComputed(format!("Failed to copy remote closure: {}\n(Proceeding with diff anyway, might fail)", e)));
                        // We don't return here, we try anyway
                    }
                }
            }
        }

        // 3. Build Local System
        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
            Line::new(
                crate::job::JobId::new(),
                "Building local system...".to_string(),
            )
            .style(LineStyle::Normal)
            .label("Diff".to_string()),
        )));

        // Use standard connection to flake
        let flake_uri = format!(
            ".#nixosConfigurations.\"{}\".config.system.build.toplevel",
            node_name.as_str()
        );

        let build_output = tokio::process::Command::new("nix")
            .arg("build")
            .arg(&flake_uri)
            .arg("--no-link")
            .arg("--print-out-paths")
            .output()
            .await;

        let local_path = match build_output {
            Ok(o) => {
                if o.status.success() {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                } else {
                    let err = String::from_utf8_lossy(&o.stderr);
                    let _ = tx.send(AppEvent::DiffComputed(format!("Build failed:\n{}", err)));
                    return;
                }
            }
            Err(e) => {
                let _ = tx.send(AppEvent::DiffComputed(format!(
                    "Failed to execute nix build: {}",
                    e
                )));
                return;
            }
        };

        // 4. Run Diff
        let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
            Line::new(crate::job::JobId::new(), "Calculating diff...".to_string())
                .style(LineStyle::Normal)
                .label("Diff".to_string()),
        )));

        // Try nvd first
        let nvd_output = tokio::process::Command::new("nvd")
            .arg("diff")
            .arg(&remote_path)
            .arg(&local_path)
            .output()
            .await;

        let diff_text = if let Ok(o) = &nvd_output {
            if o.status.success() {
                String::from_utf8_lossy(&o.stdout).to_string()
            } else {
                // If nvd fails, assume missing or error, try fallback
                println!("nvd failed, falling back..."); // Debug logging
                String::new()
            }
        } else {
            String::new()
        };

        let diff_text = if !diff_text.is_empty() {
            diff_text
        } else {
            // Fallback to nix store diff-closures
            let nix_diff = tokio::process::Command::new("nix")
                .arg("store")
                .arg("diff-closures")
                .arg(&remote_path)
                .arg(&local_path)
                .output()
                .await;

            match nix_diff {
                Ok(o) => {
                    let mut output = String::from_utf8_lossy(&o.stdout).to_string();
                    if !o.status.success() {
                        output.push_str("\n--- Stderr ---\n");
                        output.push_str(&String::from_utf8_lossy(&o.stderr));
                    }
                    output
                }
                Err(e) => format!("Both nvd and nix store diff-closures failed: {}", e),
            }
        };

        let result = format!(
            "Diff for {}\nRemote: {}\nLocal:  {}\n\n{}",
            node_name.as_str(),
            remote_path,
            local_path,
            diff_text
        );

        let _ = tx.send(AppEvent::DiffComputed(result));
    });
}
