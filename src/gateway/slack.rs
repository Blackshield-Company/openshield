//! Slack gateway — stub implementation.
//!
//! Full implementation requires Socket Mode setup with app-level tokens.
//! This stub compiles and logs events for now.

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;

/// Events emitted by the Slack bot.
#[derive(Debug, Clone)]
pub enum SlackEvent {
    UserMessage {
        channel_id: String,
        user_id: String,
        username: String,
        content: String,
    },
    Ready,
    Disconnected,
}

/// Slack bot adapter — stub.
pub struct SlackBot {
    _config: Config,
    _event_tx: mpsc::UnboundedSender<SlackEvent>,
}

impl SlackBot {
    pub fn new(config: Config, event_tx: mpsc::UnboundedSender<SlackEvent>) -> Self {
        Self {
            _config: config,
            _event_tx: event_tx,
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        info!("Slack gateway stub: would connect via Socket Mode here");
        info!("Configure with: bot_token (xoxb-...) and app_token (xapp-...)");
        // TODO: Implement full Socket Mode connection using slack-morphism
        // This requires app-level tokens and Socket Mode setup in Slack app config
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        Ok(())
    }
}

/// Spawn the Slack bot and return an event receiver.
pub fn spawn_bot(config: Config) -> mpsc::UnboundedReceiver<SlackEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let bot = SlackBot::new(config, tx.clone());
        if let Err(e) = bot.start().await {
            error!("Slack bot error: {}", e);
        }
    });

    rx
}
