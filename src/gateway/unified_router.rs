//! Unified router — normalizes events from all platforms into DiscordEvent format
//! for the existing MessageRouter.
//!
//! This is a pragmatic bridge: the MessageRouter has 1000+ lines of battle-tested
//! Discord logic (memory, skills, tool execution, multi-model). Rather than
//! duplicating that for each platform, we normalize all events to the same shape.

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::gateway::discord::DiscordEvent;
use crate::gateway::message_router::MessageRouter;
use crate::gateway::platform::PlatformEvent;
use crate::gateway::telegram::TelegramEvent;
use crate::gateway::slack::SlackEvent;
use crate::gateway::matrix::MatrixEvent;

/// Unified event router that handles all platforms.
pub struct UnifiedRouter {
    inner: MessageRouter,
}

impl UnifiedRouter {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let inner = MessageRouter::new(config)?;
        Ok(Self { inner })
    }

    /// Handle a Discord event directly.
    pub async fn handle_discord_event(&mut self, event: DiscordEvent) {
        self.inner.handle_event(event).await;
    }

    /// Handle a Telegram event by converting to DiscordEvent format.
    pub async fn handle_telegram_event(&mut self, event: TelegramEvent) {
        match event {
            TelegramEvent::UserMessage { chat_id, user_id, username, content } => {
                let (reply_tx, mut reply_rx) = mpsc::unbounded_channel();

                // Spawn a task to send replies back to Telegram
                tokio::spawn(async move {
                    // In a real implementation, this would send messages back via the Telegram bot
                    // For now, we just log the replies
                    while let Some(reply) = reply_rx.recv().await {
                        info!("Telegram reply to chat {}: {}", chat_id, reply);
                    }
                });

                let discord_event = DiscordEvent::UserMessage {
                    channel_id: chat_id as u64,
                    user_id,
                    username,
                    content,
                    reply_tx,
                };
                self.inner.handle_event(discord_event).await;
            }
            TelegramEvent::Ready => {
                info!("Telegram gateway ready");
            }
            TelegramEvent::Disconnected => {
                warn!("Telegram gateway disconnected");
            }
        }
    }

    /// Handle a Slack event by converting to DiscordEvent format.
    pub async fn handle_slack_event(&mut self, event: SlackEvent) {
        match event {
            SlackEvent::UserMessage { channel_id, user_id, username, content } => {
                let channel_id_clone = channel_id.clone();
                let (reply_tx, mut reply_rx) = mpsc::unbounded_channel();

                tokio::spawn(async move {
                    while let Some(reply) = reply_rx.recv().await {
                        info!("Slack reply to channel {}: {}", channel_id_clone, reply);
                    }
                });

                let discord_event = DiscordEvent::UserMessage {
                    channel_id: channel_id.parse().unwrap_or(0),
                    user_id: user_id.parse().unwrap_or(0),
                    username,
                    content,
                    reply_tx,
                };
                self.inner.handle_event(discord_event).await;
            }
            SlackEvent::Ready => {
                info!("Slack gateway ready");
            }
            SlackEvent::Disconnected => {
                warn!("Slack gateway disconnected");
            }
        }
    }

    /// Handle a Matrix event by converting to DiscordEvent format.
    pub async fn handle_matrix_event(&mut self, event: MatrixEvent) {
        match event {
            MatrixEvent::UserMessage { room_id, user_id, username, content } => {
                let room_id_clone = room_id.clone();
                let (reply_tx, mut reply_rx) = mpsc::unbounded_channel();

                tokio::spawn(async move {
                    while let Some(reply) = reply_rx.recv().await {
                        info!("Matrix reply to room {}: {}", room_id_clone, reply);
                    }
                });

                // Use a hash of the room_id as the channel_id since Matrix room IDs are strings
                let channel_id = hash_string_to_u64(&room_id);
                let discord_event = DiscordEvent::UserMessage {
                    channel_id,
                    user_id: hash_string_to_u64(&user_id),
                    username,
                    content,
                    reply_tx,
                };
                self.inner.handle_event(discord_event).await;
            }
            MatrixEvent::Ready => {
                info!("Matrix gateway ready");
            }
            MatrixEvent::Disconnected => {
                warn!("Matrix gateway disconnected");
            }
        }
    }
}

/// Simple hash function to convert a string to a u64 for channel/user IDs.
fn hash_string_to_u64(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}
