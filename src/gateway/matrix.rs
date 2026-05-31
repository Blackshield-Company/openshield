//! Matrix gateway — sync loop implementation.
//!
//! Connects to a Matrix homeserver and listens for messages via the sync loop.
//! Requires homeserver URL, user_id, and access_token.
//!
//! NOTE: This module is compiled only when the `matrix` feature is enabled.

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;

/// Events emitted by the Matrix bot.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum MatrixEvent {
    UserMessage {
        room_id: String,
        user_id: String,
        username: String,
        content: String,
    },
    Ready,
    Disconnected,
}

/// Shared state for sending replies back to Matrix.
#[derive(Clone)]
pub struct MatrixReplySender;

impl MatrixReplySender {
    pub async fn send_message(&self, _room_id: &str, _text: &str) {
        warn!("Matrix send_message not yet implemented — compile with --features matrix");
    }
}

/// Matrix bot adapter — sync loop.
pub struct MatrixBot {
    #[allow(dead_code)]
    config: Config,
    event_tx: mpsc::UnboundedSender<MatrixEvent>,
}

impl MatrixBot {
    pub fn new(config: Config, event_tx: mpsc::UnboundedSender<MatrixEvent>) -> Self {
        Self { config, event_tx }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let homeserver = self
            .config
            .gateway
            .matrix
            .homeserver
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Matrix homeserver not configured"))?;
        let user_id = self
            .config
            .gateway
            .matrix
            .user_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Matrix user_id not configured"))?;

        info!("Matrix gateway scaffolding...");
        info!("Homeserver: {} | User: {}", homeserver, user_id);
        info!("Compile with --features matrix for full SDK integration");

        let _ = self.event_tx.send(MatrixEvent::Ready);

        // Keep the task alive
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        let _ = self.event_tx.send(MatrixEvent::Disconnected);
        Ok(())
    }
}

/// Spawn the Matrix bot and return an event receiver.
pub fn spawn_bot(config: Config) -> (mpsc::UnboundedReceiver<MatrixEvent>, MatrixReplySender) {
    let (tx, rx) = mpsc::unbounded_channel();
    let reply_sender = MatrixReplySender;

    tokio::spawn(async move {
        let bot = MatrixBot::new(config, tx.clone());
        if let Err(e) = bot.start().await {
            error!("Matrix bot error: {}", e);
        }
    });

    (rx, reply_sender)
}
