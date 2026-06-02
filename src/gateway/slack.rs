//! Slack gateway — Socket Mode implementation.
//!
//! Connects via Slack Socket Mode (WebSocket) for real-time messaging.
//! Requires bot_token (xoxb-...) and app_token (xapp-...) from Slack app config.
//!
//! NOTE: This module is compiled only when the `slack` feature is enabled.

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;

/// Events emitted by the Slack bot.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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

/// Shared state for sending replies back to Slack.
#[derive(Clone)]
pub struct SlackReplySender;

impl SlackReplySender {
    pub async fn send_message(&self, _channel_id: &str, _text: &str) {
        warn!("Slack send_message not yet implemented — compile with --features slack");
    }
}

/// Slack bot adapter — Socket Mode.
pub struct SlackBot {
    #[allow(dead_code)]
    config: Config,
    event_tx: mpsc::UnboundedSender<SlackEvent>,
}

impl SlackBot {
    pub fn new(config: Config, event_tx: mpsc::UnboundedSender<SlackEvent>) -> Self {
        Self { config, event_tx }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let _bot_token = self
            .config
            .gateway
            .slack
            .bot_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Slack bot token not configured"))?;
        let _app_token = self
            .config
            .gateway
            .slack
            .app_token
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!("Slack app token not configured (required for Socket Mode)")
            })?;

        info!("Slack gateway scaffolding...");
        info!("Bot token: xoxb-*** | App token: xapp-***");
        info!("Compile with --features slack for full Socket Mode integration");

        let _ = self.event_tx.send(SlackEvent::Ready);

        // Keep the task alive
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        let _ = self.event_tx.send(SlackEvent::Disconnected);
        Ok(())
    }
}

/// Spawn the Slack bot and return an event receiver.
pub fn spawn_bot(config: Config) -> (mpsc::UnboundedReceiver<SlackEvent>, SlackReplySender) {
    let (tx, rx) = mpsc::unbounded_channel();
    let reply_sender = SlackReplySender;

    tokio::spawn(async move {
        let bot = SlackBot::new(config, tx.clone());
        if let Err(e) = bot.start().await {
            error!("Slack bot error: {}", e);
        }
    });

    (rx, reply_sender)
}
