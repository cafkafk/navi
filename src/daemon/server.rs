use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

use crate::command::tui::model::DeploySettings;
use crate::daemon::protocol::{DaemonEvent, DaemonStateSnapshot, Request, Response};
use crate::error::{NaviError, NaviResult};
use crate::nix::{
    deployment::{Deployment, EvaluatorType, Options, ParallelismLimit},
    host::{CopyDirection, CopyOptions, Host, Local},
    Goal, Hive, HivePath, NixFlags, NodeName, NodeState, StorePath,
};

pub struct DaemonState {
    pub node_states: HashMap<NodeName, NodeState>,
    pub active_tasks: HashMap<Uuid, String>,
    pub logs: Vec<String>,
    pub connected_clients: usize,
    pub last_activity: std::time::Instant,
}

pub struct DaemonServer {
    state: Arc<RwLock<DaemonState>>,
    event_tx: broadcast::Sender<DaemonEvent>,
}

impl DaemonServer {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            state: Arc::new(RwLock::new(DaemonState {
                node_states: HashMap::new(),
                active_tasks: HashMap::new(),
                logs: Vec::new(),
                connected_clients: 0,
                last_activity: std::time::Instant::now(),
            })),
            event_tx,
        }
    }

    pub async fn run(&self) -> NaviResult<()> {
        let socket_path = "/tmp/navi.sock"; // TODO: Use XDG Runtime
        if std::path::Path::new(socket_path).exists() {
            tokio::fs::remove_file(socket_path).await.ok();
        }

        let listener = UnixListener::bind(socket_path)?;
        // println!("Daemon listening on {}", socket_path);

        /// Lifecycle Management Philosophy:
        /// 1. Lazy Start: The TUI spawns the daemon if it's not running.
        /// 2. Persistence: The daemon runs in its own process group, surviving external TUI termination (SIGINT).
        /// 3. Auto-Shutdown: To conserve resources, the daemon monitors itself. If:
        ///    - No clients are connected (connected_clients == 0)
        ///    - No tasks are active (active_tasks.is_empty())
        ///    - This idle state persists for >10 seconds
        ///    Then the daemon voluntarily exits.
        let state_monitor = self.state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let s = state_monitor.read().await;

                let is_idle = s.connected_clients == 0 && s.active_tasks.is_empty();
                // 10 seconds of pure idleness causes exit
                if is_idle && s.last_activity.elapsed() > Duration::from_secs(10) {
                    std::process::exit(0);
                }
            }
        });

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let state = self.state.clone();
                    let tx = self.event_tx.clone();
                    let rx = self.event_tx.subscribe();

                    // Increment client count
                    {
                        let mut s = state.write().await;
                        s.connected_clients += 1;
                        s.last_activity = std::time::Instant::now();
                    }

                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, state.clone(), tx, rx).await {
                            eprintln!("Client disconnected: {}", e);
                        }
                        // Decrement client count
                        {
                            let mut s = state.write().await;
                            if s.connected_clients > 0 {
                                s.connected_clients -= 1;
                            }
                            s.last_activity = std::time::Instant::now();
                        }
                    });
                }
                Err(e) => eprintln!("Accept error: {}", e),
            }
        }
    }
}

async fn handle_client(
    stream: UnixStream,
    state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<DaemonEvent>,
    mut rx: broadcast::Receiver<DaemonEvent>,
) -> NaviResult<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // Channel to send direct responses from Request handler to Writer
    let (resp_tx, mut resp_rx) = mpsc::channel::<Response>(32);

    // Writer Task
    let write_task = tokio::spawn(async move {
        loop {
            let response = tokio::select! {
                Ok(event) = rx.recv() => Response::Event(event),
                Some(resp) = resp_rx.recv() => resp,
                else => break, // All channels closed
            };

            let mut s = serde_json::to_string(&response).unwrap();
            s.push('\n');
            if writer.write_all(s.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    // Reader Loop
    while let Ok(Some(line)) = lines.next_line().await {
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<Request>(&line) {
            Ok(req) => {
                let resp = process_request(req, &state, &tx).await;
                if resp_tx.send(resp).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                let _ = resp_tx
                    .send(Response::Error(format!("Invalid request: {}", e)))
                    .await;
            }
        }
    }

    write_task.abort();
    Ok(())
}

async fn load_hive() -> NaviResult<Hive> {
    let cwd = std::env::current_dir()?;
    let flake_path = cwd.join("flake.nix");
    let hive_path = cwd.join("hive.nix");

    let path = if flake_path.exists() {
        HivePath::from_path(flake_path).await?
    } else if hive_path.exists() {
        HivePath::from_path(hive_path).await?
    } else {
        return Err(NaviError::Unknown {
            message: "No flake.nix or hive.nix found in current directory".to_string(),
        });
    };
    Hive::new(path).await
}

async fn run_deployment(
    _task_id: Uuid,
    nodes: Vec<NodeName>,
    settings: DeploySettings,
    parallel: usize,
    tx: broadcast::Sender<DaemonEvent>,
) -> NaviResult<()> {
    let hive = load_hive().await?;
    let configs = hive.deployment_info_selected(&nodes).await?;

    let mut targets = HashMap::new();
    for (name, config) in configs {
        let host = config.to_ssh_host().map(|h| h.upcast());
        targets.insert(
            name.clone(),
            crate::nix::deployment::TargetNode::new(name, host, config),
        );
    }

    let (prog_tx, mut prog_rx) = mpsc::unbounded_channel();
    let tx_bridge = tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = prog_rx.recv().await {
            use crate::progress::{LineStyle, Message};
            match msg {
                Message::Print(line) | Message::PrintMeta(line) => {
                    let text = line.text.clone();
                    let label = line.label.clone();
                    if !label.is_empty() {
                        if let Ok(node) = NodeName::new(label.clone()) {
                            match line.style {
                                LineStyle::Success | LineStyle::SuccessNoop => {
                                    let _ = tx_bridge.send(DaemonEvent::NodeStateChanged(
                                        node.clone(),
                                        NodeState::Success(text.clone()),
                                    ));
                                }
                                LineStyle::Failure => {
                                    let _ = tx_bridge.send(DaemonEvent::NodeStateChanged(
                                        node.clone(),
                                        NodeState::Failed(text.clone()),
                                    ));
                                }
                                _ => {
                                    let _ = tx_bridge
                                        .send(DaemonEvent::NodeLog(node.clone(), text.clone()));
                                }
                            }
                        } else {
                            let _ =
                                tx_bridge.send(DaemonEvent::Log(format!("{} | {}", label, text)));
                        }
                    } else {
                        let _ = tx_bridge.send(DaemonEvent::Log(text));
                    }
                }
                Message::Complete => break,
                _ => {}
            }
        }
    });

    let mut deployment = Deployment::new(hive, targets, Goal::Switch, Some(prog_tx));

    let mut options = Options::default();
    options.set_upload_keys(!settings.no_keys);
    options.set_substituters_push(!settings.no_substitute);
    options.set_gzip(!settings.no_gzip);
    options.set_reboot(settings.reboot);
    options.set_install_bootloader(settings.install_bootloader);
    if settings.build_on_target {
        options.set_force_build_on_target(true);
    }
    options.set_force_replace_unknown_profiles(settings.force_replace_unknown_profiles);
    options.set_create_gc_roots(settings.keep_result);
    options.set_evaluator(EvaluatorType::Chunked);

    deployment.set_options(options);

    let mut limit = ParallelismLimit::default();
    limit.set_apply_limit(if parallel == 0 { nodes.len() } else { parallel });
    deployment.set_parallelism_limit(limit);

    deployment.execute().await?;

    Ok(())
}

async fn run_gc(
    nodes: Vec<NodeName>,
    interval: Option<String>,
    tx: broadcast::Sender<DaemonEvent>,
) -> NaviResult<()> {
    let hive = load_hive().await?;
    let configs = hive.deployment_info_selected(&nodes).await?;
    let flags = hive.nix_flags();

    let mut tasks = Vec::new();

    for (name, config) in configs {
        let tx = tx.clone();
        let interval = interval.clone();
        let flags = flags.clone();

        tasks.push(tokio::spawn(async move {
            let mut host: Box<dyn Host> = if let Some(ssh) = config.to_ssh_host() {
                ssh.upcast()
            } else {
                Local::new(flags).upcast()
            };

            let mut args = vec!["nix-collect-garbage"];
            let interval_arg;
            if let Some(ref i) = interval {
                args.push("--delete-older-than");
                interval_arg = i.clone();
                args.push(&interval_arg);
            } else {
                args.push("-d");
            }

            let _ = tx.send(DaemonEvent::NodeLog(
                name.clone(),
                "Starting GC...".to_string(),
            ));
            let _ = tx.send(DaemonEvent::NodeStateChanged(
                name.clone(),
                NodeState::Running("GC...".to_string()),
            ));

            match host.run_command(&args).await {
                Ok(_) => {
                    let _ = tx.send(DaemonEvent::NodeStateChanged(
                        name.clone(),
                        NodeState::Success("GC Completed".to_string()),
                    ));
                }
                Err(e) => {
                    let _ = tx.send(DaemonEvent::NodeStateChanged(
                        name.clone(),
                        NodeState::Failed(format!("GC Failed: {}", e)),
                    ));
                }
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    Ok(())
}

async fn run_diff(node_name: NodeName, tx: broadcast::Sender<DaemonEvent>) -> NaviResult<()> {
    let hive = load_hive().await?;
    let config = match hive.deployment_info_single(&node_name).await? {
        Some(c) => c,
        None => {
            return Err(NaviError::Unknown {
                message: "Node not found".to_string(),
            })
        }
    };

    let host_str = config.target_host.as_deref().unwrap_or("localhost");
    let user = config.target_user.as_deref().unwrap_or("root");
    let target = if host_str == "localhost" {
        "localhost".to_string()
    } else {
        format!("{}@{}", user, host_str)
    };

    let _ = tx.send(DaemonEvent::Log(format!(
        "Fetching remote state from {}...",
        target
    )));

    let remote_path_string = if host_str == "localhost" {
        tokio::fs::read_link("/run/current-system")
            .await
            .map(|p| p.to_string_lossy().to_string())
    } else {
        let output = tokio::process::Command::new("ssh")
            .arg(&target)
            .arg("readlink -f /run/current-system")
            .output()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                String::from_utf8_lossy(&output.stderr),
            ))
        }
    };

    let remote_path = match remote_path_string {
        Ok(p) => p,
        Err(e) => {
            return Err(NaviError::Unknown {
                message: format!("Failed to fetch remote system path: {}", e),
            })
        }
    };

    if host_str != "localhost" {
        let _ = tx.send(DaemonEvent::Log(
            "Copying remote closure info...".to_string(),
        ));
        if let Some(ssh) = config.to_ssh_host() {
            let mut host = ssh.upcast();
            if let Ok(store_path) = StorePath::try_from(remote_path.clone()) {
                let copy_opts = CopyOptions::default().include_outputs(true);
                if let Err(e) = host
                    .copy_closure(&store_path, CopyDirection::FromRemote, copy_opts)
                    .await
                {
                    let _ = tx.send(DaemonEvent::Log(format!(
                        "Warning: Failed to copy remote closure: {}",
                        e
                    )));
                }
            }
        }
    }

    let _ = tx.send(DaemonEvent::Log("Building local system...".to_string()));
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
        .await
        .map_err(|e| NaviError::Unknown {
            message: e.to_string(),
        })?;

    if !build_output.status.success() {
        return Err(NaviError::Unknown {
            message: format!(
                "Build failed: {}",
                String::from_utf8_lossy(&build_output.stderr)
            ),
        });
    }
    let local_path = String::from_utf8_lossy(&build_output.stdout)
        .trim()
        .to_string();

    let _ = tx.send(DaemonEvent::Log("Calculating diff...".to_string()));

    // Try nvd logic or fallback as in TUI
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
            String::new()
        }
    } else {
        String::new()
    };

    let final_diff = if !diff_text.is_empty() {
        diff_text
    } else {
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
        final_diff
    );

    let _ = tx.send(DaemonEvent::DiffComputed(result));
    Ok(())
}

async fn process_request(
    req: Request,
    state: &Arc<RwLock<DaemonState>>,
    tx: &broadcast::Sender<DaemonEvent>,
) -> Response {
    match req {
        Request::GetState => {
            let s = state.read().await;
            Response::State(DaemonStateSnapshot {
                node_states: s.node_states.clone(),
                active_tasks: s.active_tasks.clone(),
                logs: s.logs.clone(),
            })
        }
        Request::Deploy {
            nodes,
            settings,
            parallel,
        } => {
            let uuid = Uuid::new_v4();
            let description = format!("Deploying {} nodes", nodes.len());
            {
                let mut s = state.write().await;
                s.active_tasks.insert(uuid, description.clone());
            }
            let _ = tx.send(DaemonEvent::TaskStarted(uuid, description));
            let tx_inner = tx.clone();
            let state_inner = state.clone();
            tokio::spawn(async move {
                match run_deployment(uuid, nodes, settings, parallel, tx_inner.clone()).await {
                    Ok(_) => {
                        let _ = tx_inner.send(DaemonEvent::Log(
                            "Deployment completed successfully.".to_string(),
                        ));
                    }
                    Err(e) => {
                        let _ =
                            tx_inner.send(DaemonEvent::Log(format!("Deployment failed: {}", e)));
                    }
                }
                let mut s = state_inner.write().await;
                s.active_tasks.remove(&uuid);
                let _ = tx_inner.send(DaemonEvent::TaskFinished(uuid));
            });
            Response::Ok
        }
        Request::Diff { node } => {
            let uuid = Uuid::new_v4();
            let description = format!("Diffing {}", node.as_str());
            {
                let mut s = state.write().await;
                s.active_tasks.insert(uuid, description.clone());
            }
            let _ = tx.send(DaemonEvent::TaskStarted(uuid, description));
            let tx_inner = tx.clone();
            let state_inner = state.clone();
            tokio::spawn(async move {
                match run_diff(node, tx_inner.clone()).await {
                    Ok(_) => {}
                    Err(e) => {
                        let _ =
                            tx_inner.send(DaemonEvent::DiffComputed(format!("Diff failed: {}", e)));
                    }
                }
                let mut s = state_inner.write().await;
                s.active_tasks.remove(&uuid);
                let _ = tx_inner.send(DaemonEvent::TaskFinished(uuid));
            });
            Response::Ok
        }
        Request::GarbageCollect { nodes, interval } => {
            let uuid = Uuid::new_v4();
            let description = format!("Garbage Collect ({} nodes)", nodes.len());
            {
                let mut s = state.write().await;
                s.active_tasks.insert(uuid, description.clone());
            }
            let _ = tx.send(DaemonEvent::TaskStarted(uuid, description));
            let tx_inner = tx.clone();
            let state_inner = state.clone();
            tokio::spawn(async move {
                match run_gc(nodes, interval, tx_inner.clone()).await {
                    Ok(_) => {
                        let _ = tx_inner.send(DaemonEvent::Log("GC completed.".to_string()));
                    }
                    Err(e) => {
                        let _ = tx_inner.send(DaemonEvent::Log(format!("GC failed: {}", e)));
                    }
                }
                let mut s = state_inner.write().await;
                s.active_tasks.remove(&uuid);
                s.last_activity = std::time::Instant::now();
                let _ = tx_inner.send(DaemonEvent::TaskFinished(uuid));
            });
            Response::Ok
        }
    }
}
