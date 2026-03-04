use crate::command::tui::events::AppEvent;
use crate::command::tui::loading;
use crate::command::tui::model::DeploySettings;
use crate::daemon::client::DaemonClient;
use crate::nix::hive::HivePath;
use crate::nix::NodeName;
/// This module abstracts the execution engine (Local vs Daemon)
use tokio::sync::mpsc::UnboundedSender;

pub async fn start_deployment(
    hive_path: HivePath,
    nodes: Vec<NodeName>,
    settings: DeploySettings,
    parallel: usize,
    tx: UnboundedSender<AppEvent>,
    daemon: Option<&DaemonClient>,
) {
    if let Some(client) = daemon {
        // Run via Daemon
        if let Err(e) = client.deploy(nodes, settings, parallel).await {
            let _ = tx.send(AppEvent::DiffComputed(format!(
                "Failed to start deployment via daemon: {}",
                e
            )));
        }
    } else {
        // Run Locally
        loading::start_deployment(hive_path, nodes, parallel, tx, settings);
    }
}

pub async fn start_diff(
    hive_path: HivePath,
    node: NodeName,
    tx: UnboundedSender<AppEvent>,
    daemon: Option<&DaemonClient>,
) {
    if let Some(client) = daemon {
        // Run via Daemon
        if let Err(e) = client.diff(node).await {
            let _ = tx.send(AppEvent::DiffComputed(format!(
                "Failed to start diff via daemon: {}",
                e
            )));
        }
    } else {
        // Run Locally
        loading::start_diff(hive_path, node, tx);
    }
}

pub async fn start_garbage_collection(
    hive_path: HivePath,
    nodes: Vec<NodeName>,
    interval: Option<String>,
    tx: UnboundedSender<AppEvent>,
    daemon: Option<&DaemonClient>,
) {
    if let Some(client) = daemon {
        // Run via Daemon
        if let Err(e) = client.garbage_collect(nodes, interval).await {
            // Just send a progress message
            use crate::progress::Message;
            use crate::progress::{Line, LineStyle};
            let _ = tx.send(AppEvent::Progress(Message::PrintMeta(
                Line::new(
                    crate::job::JobId::new(),
                    format!("Failed to start GC via daemon: {}", e),
                )
                .style(LineStyle::Failure)
                .label("GC".to_string()),
            )));
        }
    } else {
        // Run Locally
        loading::start_garbage_collection(hive_path, nodes, interval, tx);
    }
}
