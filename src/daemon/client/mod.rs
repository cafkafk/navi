use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::protocol::{DaemonEvent, Request, Response};
use crate::command::tui::model::DeploySettings;
use crate::error::NaviResult;
use crate::nix::NodeName;

#[derive(Clone)]
pub struct DaemonClient {
    tx: mpsc::Sender<Request>,
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Response>>>,
}

impl DaemonClient {
    pub async fn connect(autostart: bool) -> NaviResult<Self> {
        let socket_path = "/tmp/navi.sock"; // TODO: XDG Runtime

        // Loop to allow retries if autostarting
        let mut attempts = 0;
        let stream = loop {
            match UnixStream::connect(socket_path).await {
                Ok(s) => break s,
                Err(e) => {
                    if !autostart {
                        return Err(crate::error::NaviError::DaemonConnectionError {
                            message: format!("Failed to connect to daemon: {}", e),
                        });
                    }

                    let should_retry = e.kind() == std::io::ErrorKind::ConnectionRefused
                        || e.kind() == std::io::ErrorKind::NotFound;
                    if should_retry && attempts == 0 {
                        // Try to start the daemon
                        let exe = std::env::current_exe().map_err(|e| {
                            crate::error::NaviError::Unknown {
                                message: e.to_string(),
                            }
                        })?;

                        use std::os::unix::process::CommandExt;
                        std::process::Command::new(exe)
                            .arg("daemon")
                            .arg("start")
                            .process_group(0)
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn()
                            .map_err(|e| crate::error::NaviError::Unknown {
                                message: format!("Failed to spawn daemon: {}", e),
                            })?;

                        // Give it a moment to start
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        attempts += 1;
                        continue;
                    } else if should_retry && attempts < 20 {
                        // Wait and retry
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        attempts += 1;
                        continue;
                    }

                    return Err(crate::error::NaviError::DaemonConnectionError {
                        message: format!("Failed to connect to daemon (autostart failed): {}", e),
                    });
                }
            }
        };

        let (read_half, mut write_half) = stream.into_split();
        let (req_tx, mut req_rx) = mpsc::channel::<Request>(32);
        let (resp_tx, resp_rx) = mpsc::channel::<Response>(100);

        // Writer Task
        tokio::spawn(async move {
            while let Some(req) = req_rx.recv().await {
                if let Ok(s) = serde_json::to_string(&req) {
                    let mut line = s;
                    line.push('\n');
                    if write_half.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        });

        // Reader Task
        let resp_tx_clone = resp_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(read_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.is_empty() {
                    continue;
                }
                if let Ok(resp) = serde_json::from_str::<Response>(&line) {
                    if resp_tx_clone.send(resp).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            tx: req_tx,
            rx: Arc::new(tokio::sync::Mutex::new(resp_rx)),
        })
    }

    pub async fn send(&self, req: Request) -> NaviResult<()> {
        self.tx
            .send(req)
            .await
            .map_err(|_| crate::error::NaviError::DaemonConnectionError {
                message: "Send failed".to_string(),
            })
    }

    pub async fn next_response(&self) -> Option<Response> {
        self.rx.lock().await.recv().await
    }

    pub async fn get_status(&self) -> NaviResult<super::protocol::DaemonStateSnapshot> {
        self.send(Request::GetState).await?;

        let mut attempts = 0;
        loop {
            match self.next_response().await {
                Some(Response::State(snapshot)) => {
                    return Ok(snapshot);
                }
                Some(Response::Error(e)) => {
                    return Err(crate::error::NaviError::DaemonError { message: e });
                }
                Some(_) => {
                    // Ignore other events
                }
                None => {
                    return Err(crate::error::NaviError::DaemonConnectionError {
                        message: "Daemon disconnected".to_string(),
                    });
                }
            }
            attempts += 1;
            if attempts > 50 {
                return Err(crate::error::NaviError::DaemonConnectionError {
                    message: "Timeout waiting for status".to_string(),
                });
            }
        }
    }

    pub async fn deploy(
        &self,
        nodes: Vec<NodeName>,
        settings: DeploySettings,
        parallel: usize,
    ) -> NaviResult<()> {
        self.send(Request::Deploy {
            nodes,
            settings,
            parallel,
        })
        .await
    }

    pub async fn diff(&self, node: NodeName) -> NaviResult<()> {
        self.send(Request::Diff { node }).await
    }

    pub async fn garbage_collect(
        &self,
        nodes: Vec<NodeName>,
        interval: Option<String>,
    ) -> NaviResult<()> {
        self.send(Request::GarbageCollect { nodes, interval }).await
    }

    // Helper to drain event queue for the TUI main loop
    pub async fn try_next_event(&self) -> Option<DaemonEvent> {
        None
    }

    pub fn split(self) -> (DaemonClientSender, DaemonClientReceiver) {
        (
            DaemonClientSender { tx: self.tx },
            DaemonClientReceiver { rx: self.rx },
        )
    }
}

#[derive(Clone)]
pub struct DaemonClientSender {
    tx: mpsc::Sender<Request>,
}

impl DaemonClientSender {
    pub async fn send(&self, req: Request) -> NaviResult<()> {
        self.tx
            .send(req)
            .await
            .map_err(|_| crate::error::NaviError::DaemonConnectionError {
                message: "Send failed".to_string(),
            })
    }

    pub async fn deploy(
        &self,
        nodes: Vec<NodeName>,
        settings: DeploySettings,
        parallel: usize,
    ) -> NaviResult<()> {
        self.send(Request::Deploy {
            nodes,
            settings,
            parallel,
        })
        .await
    }

    pub async fn diff(&self, node: NodeName) -> NaviResult<()> {
        self.send(Request::Diff { node }).await
    }

    pub async fn garbage_collect(
        &self,
        nodes: Vec<NodeName>,
        interval: Option<String>,
    ) -> NaviResult<()> {
        self.send(Request::GarbageCollect { nodes, interval }).await
    }

    pub async fn get_status_oneshot(&self) -> NaviResult<()> {
        self.send(Request::GetState).await
    }
}

pub struct DaemonClientReceiver {
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Response>>>,
}

impl DaemonClientReceiver {
    pub async fn next(&self) -> Option<Response> {
        self.rx.lock().await.recv().await
    }
}
