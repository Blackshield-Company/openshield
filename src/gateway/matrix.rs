//! Matrix gateway — stub implementation.
//!
//! Full implementation requires homeserver URL, user_id, and access_token.
//! This stub compiles and logs events for now.

use tokio::sync::mpsc;
use tracing::{error, info};

use crate::config::Config;

/// Events emitted by the Matrix bot.
#[derive(Debug, Clone)]
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

/// Matrix bot adapter — stub.
pub struct MatrixBot {
    _config: Config,
    _event_tx: mpsc::UnboundedSender<MatrixEvent>,
}

impl MatrixBot {
    pub fn new(config: Config, event_tx: mpsc::UnboundedSender<MatrixEvent>) -> Self {
        Self {
            _config: config,
            _event_tx: event_tx,
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        info!("Matrix gateway stub: would connect to homeserver here");
        info!("Configure with: homeserver URL, user_id, and access_token");
        // TODO: Implement full Matrix sync loop using matrix-sdk
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        Ok(())
    }
}

/// Spawn the Matrix bot and return an event receiver.
pub fn spawn_bot(config: Config) -> mpsc::UnboundedReceiver<MatrixEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let bot = MatrixBot::new(config, tx.clone());
        if let Err(e) = bot.start().await {
            error!("Matrix bot error: {}", e);
        }
    });

    rx
}
