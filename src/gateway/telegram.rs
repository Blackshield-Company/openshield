//! Telegram gateway using teloxide.
//!
//! Receives messages from Telegram Bot API, routes them through the
//! unified event system.

use anyhow::{Context, Result};
use teloxide::prelude::*;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;

/// Events emitted by the Telegram bot.
#[derive(Debug, Clone)]
pub enum TelegramEvent {
    UserMessage {
        chat_id: i64,
        user_id: u64,
        username: String,
        content: String,
    },
    Ready,
    Disconnected,
}

/// Telegram bot adapter.
pub struct TelegramBot {
    config: Config,
    event_tx: mpsc::UnboundedSender<TelegramEvent>,
}

impl TelegramBot {
    pub fn new(config: Config, event_tx: mpsc::UnboundedSender<TelegramEvent>) -> Self {
        Self { config, event_tx }
    }

    pub async fn start(&self) -> Result<()> {
        let token = self
            .config
            .gateway
            .telegram
            .bot_token
            .as_ref()
            .context("Telegram bot token not configured")?;

        info!("Telegram bot starting...");

        let bot = Bot::new(token);
        let allowed_chats = self.config.gateway.telegram.allowed_chats.clone();
        let require_prefix = self.config.gateway.telegram.require_command_prefix;
        let event_tx = self.event_tx.clone();

        let handler = Update::filter_message().endpoint(
            move |bot: Bot, msg: Message| {
                let event_tx = event_tx.clone();
                let allowed_chats = allowed_chats.clone();
                async move {
                    handle_message(bot, msg, event_tx, allowed_chats, require_prefix).await;
                    Ok::<(), teloxide::RequestError>(())
                }
            },
        );

        Dispatcher::builder(bot, handler)
            .build()
            .dispatch()
            .await;

        Ok(())
    }
}

async fn handle_message(
    _bot: Bot,
    msg: Message,
    event_tx: mpsc::UnboundedSender<TelegramEvent>,
    allowed_chats: Vec<i64>,
    require_prefix: bool,
) {
    let text = match msg.text() {
        Some(t) => t,
        None => return,
    };

    let chat_id = msg.chat.id.0;

    if !allowed_chats.is_empty() && !allowed_chats.contains(&chat_id) {
        return;
    }

    if require_prefix && !text.starts_with('/') {
        return;
    }

    let user = msg.from.clone();
    let username = user.as_ref().map(|u| {
        u.username.clone().unwrap_or_else(|| {
            format!("{} {}", u.first_name, u.last_name.clone().unwrap_or_default())
                .trim()
                .to_string()
        })
    }).unwrap_or_else(|| "Unknown".to_string());

    let user_id = user.map(|u| u.id.0 as u64).unwrap_or(0);

    let event = TelegramEvent::UserMessage {
        chat_id,
        user_id,
        username,
        content: text.to_string(),
    };

    if let Err(e) = event_tx.send(event) {
        error!("Failed to send Telegram event: {}", e);
    }
}

/// Spawn the Telegram bot and return an event receiver.
pub fn spawn_bot(config: Config) -> mpsc::UnboundedReceiver<TelegramEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let bot = TelegramBot::new(config, tx.clone());
        if let Err(e) = bot.start().await {
            error!("Telegram bot error: {}", e);
        }
    });

    rx
}
