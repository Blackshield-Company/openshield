//! Unified router — normalizes events from all platforms into GatewayEvent format
//! for the existing MessageRouter.
//!
//! This is a pragmatic bridge: the MessageRouter has 1000+ lines of battle-tested
//! routing logic (memory, skills, tool execution, multi-model). Rather than
//! duplicating that for each platform, we normalize all events to the same shape.

use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::config::Config;
use crate::gateway::events::GatewayEvent;
use crate::gateway::message_router::MessageRouter;

/// Unified event router that handles all platforms.
pub struct UnifiedRouter {
    inner: MessageRouter,
}

impl UnifiedRouter {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let inner = MessageRouter::new(config)?;
        Ok(Self { inner })
    }

    /// Handle a generic gateway event directly.
    #[allow(dead_code)]
    pub async fn handle_event(&mut self, event: GatewayEvent) {
        self.inner.handle_event(event).await;
    }

    /// Handle a Discord event directly.
    #[cfg(feature = "discord")]
    #[allow(dead_code)]
    pub async fn handle_discord_event(&mut self, event: crate::gateway::discord::DiscordEvent) {
        match event {
            crate::gateway::discord::DiscordEvent::UserMessage {
                channel_id,
                user_id,
                username,
                content,
                reply_tx,
            } => {
                let gateway_event = GatewayEvent::UserMessage {
                    channel_id,
                    user_id,
                    username,
                    content,
                    reply_tx,
                };
                self.inner.handle_event(gateway_event).await;
            }
            crate::gateway::discord::DiscordEvent::SlashCommand {
                interaction,
                reply_tx,
            } => {
                self.inner
                    .handle_discord_interaction(interaction, reply_tx)
                    .await;
            }
            crate::gateway::discord::DiscordEvent::Ready => {
                info!("Discord gateway ready");
            }
            crate::gateway::discord::DiscordEvent::Disconnected => {
                warn!("Discord gateway disconnected");
            }
        }
    }

    /// Handle a Telegram event by converting to GatewayEvent format.
    #[cfg(feature = "telegram")]
    pub async fn handle_telegram_event(
        &mut self,
        event: crate::gateway::telegram::TelegramEvent,
        reply_sender: &crate::gateway::telegram::TelegramReplySender,
    ) {
        match event {
            crate::gateway::telegram::TelegramEvent::UserMessage {
                chat_id,
                user_id,
                username,
                content,
            } => {
                let (reply_tx, mut reply_rx): (
                    mpsc::UnboundedSender<String>,
                    mpsc::UnboundedReceiver<String>,
                ) = mpsc::unbounded_channel();

                // Spawn a task to send replies back to Telegram
                let sender = reply_sender.clone();
                tokio::spawn(async move {
                    while let Some(reply) = reply_rx.recv().await {
                        sender.send_message(chat_id, &reply).await;
                    }
                });

                let discord_event = GatewayEvent::UserMessage {
                    channel_id: chat_id as u64,
                    user_id,
                    username,
                    content,
                    reply_tx,
                };
                self.inner.handle_event(discord_event).await;
            }
            crate::gateway::telegram::TelegramEvent::Ready => {
                info!("Telegram gateway ready");
            }
            crate::gateway::telegram::TelegramEvent::Disconnected => {
                warn!("Telegram gateway disconnected");
            }
        }
    }

    /// Handle a Slack event by converting to GatewayEvent format.
    #[cfg(feature = "slack")]
    pub async fn handle_slack_event(
        &mut self,
        event: crate::gateway::slack::SlackEvent,
        reply_sender: &crate::gateway::slack::SlackReplySender,
    ) {
        match event {
            crate::gateway::slack::SlackEvent::UserMessage {
                channel_id,
                user_id,
                username,
                content,
            } => {
                let channel_id_clone = channel_id.clone();
                let (reply_tx, mut reply_rx): (
                    mpsc::UnboundedSender<String>,
                    mpsc::UnboundedReceiver<String>,
                ) = mpsc::unbounded_channel();

                let sender = reply_sender.clone();
                tokio::spawn(async move {
                    while let Some(reply) = reply_rx.recv().await {
                        sender.send_message(&channel_id_clone, &reply).await;
                    }
                });

                let gateway_event = GatewayEvent::UserMessage {
                    channel_id: hash_string_to_u64(&channel_id),
                    user_id: hash_string_to_u64(&user_id),
                    username,
                    content,
                    reply_tx,
                };
                self.inner.handle_event(gateway_event).await;
            }
            crate::gateway::slack::SlackEvent::Ready => {
                info!("Slack gateway ready");
            }
            crate::gateway::slack::SlackEvent::Disconnected => {
                warn!("Slack gateway disconnected");
            }
        }
    }

    /// Handle a Matrix event by converting to GatewayEvent format.
    pub async fn handle_matrix_event(
        &mut self,
        event: crate::gateway::matrix::MatrixEvent,
        reply_sender: &crate::gateway::matrix::MatrixReplySender,
    ) {
        match event {
            crate::gateway::matrix::MatrixEvent::UserMessage {
                room_id,
                user_id,
                username,
                content,
            } => {
                let room_id_clone = room_id.clone();
                let (reply_tx, mut reply_rx): (
                    mpsc::UnboundedSender<String>,
                    mpsc::UnboundedReceiver<String>,
                ) = mpsc::unbounded_channel();

                let sender = reply_sender.clone();
                tokio::spawn(async move {
                    while let Some(reply) = reply_rx.recv().await {
                        sender.send_message(&room_id_clone, &reply).await;
                    }
                });

                let channel_id = hash_string_to_u64(&room_id);
                let gateway_event = GatewayEvent::UserMessage {
                    channel_id,
                    user_id: hash_string_to_u64(&user_id),
                    username,
                    content,
                    reply_tx,
                };
                self.inner.handle_event(gateway_event).await;
            }
            crate::gateway::matrix::MatrixEvent::Ready => {
                info!("Matrix gateway ready");
            }
            crate::gateway::matrix::MatrixEvent::Disconnected => {
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
