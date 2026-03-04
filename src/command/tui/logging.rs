use async_trait::async_trait;
use std::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;

use crate::error::NaviResult;
use crate::progress::{Line, LineStyle, Message, ProgressOutput, Sender as ProgressSender};

struct LogWriter {
    sender: UnboundedSender<Message>,
}

impl std::io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf).trim().to_string();
        if !text.is_empty() {
            let _ = self.sender.send(Message::PrintMeta(
                Line::new(crate::job::JobId::new(), text)
                    .style(LineStyle::Normal)
                    .label("System".to_string()),
            ));
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn init_tui_logging(sender: UnboundedSender<Message>) {
    let writer = LogWriter { sender };
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(writer))
        .with_target(false)
        .with_level(false)
        .without_time()
        .with_ansi(false)
        .init();
}

pub struct TuiOutput {
    pub sender: UnboundedSender<Message>,
}

#[async_trait]
impl ProgressOutput for TuiOutput {
    async fn run_until_completion(self) -> NaviResult<Self> {
        Ok(self)
    }

    fn get_sender(&mut self) -> Option<ProgressSender> {
        Some(self.sender.clone())
    }
}
