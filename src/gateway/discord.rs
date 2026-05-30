use anyhow::{Context, Result};
use serenity::async_trait;
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseFollowup, CreateInteractionResponseMessage};
use serenity::client::{Context as SerenityContext, EventHandler};
use serenity::all::Interaction;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;
use serenity::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::gateway::commands::register_commands;
use crate::gateway::message_router::MessageRouter;

/// Events emitted by the Discord bot for the main application to handle.
#[derive(Debug)]
pub enum DiscordEvent {
    /// A user sent a message that should be processed.
    UserMessage {
        channel_id: u64,
        user_id: u64,
        username: String,
        content: String,
        reply_tx: mpsc::UnboundedSender<String>,
    },
    /// A slash command was invoked.
    SlashCommand {
        interaction: Interaction,
        reply_tx: mpsc::UnboundedSender<String>,
    },
    /// Bot is ready.
    Ready,
    /// Bot disconnected.
    Disconnected,
}

/// Discord bot adapter using serenity.
pub struct DiscordBot {
    config: Config,
    event_tx: mpsc::UnboundedSender<DiscordEvent>,
}

impl DiscordBot {
    pub fn new(config: Config, event_tx: mpsc::UnboundedSender<DiscordEvent>) -> Self {
        Self { config, event_tx }
    }

    /// Start the Discord bot. Blocks until shutdown.
    pub async fn start(&self) -> Result<()> {
        let discord_config = &self.config.gateway.discord;

        let token = discord_config
            .bot_token
            .as_ref()
            .context("Discord bot token not configured")?;

        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let handler = Handler {
            event_tx: self.event_tx.clone(),
            config: self.config.clone(),
        };

        let mut client = Client::builder(token, intents)
            .event_handler(handler)
            .await
            .context("Failed to create Discord client")?;

        info!("Discord bot starting...");
        client.start().await.context("Discord client error")?;

        Ok(())
    }
}

struct Handler {
    event_tx: mpsc::UnboundedSender<DiscordEvent>,
    config: Config,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: SerenityContext, msg: Message) {
        // Ignore bot messages
        if msg.author.bot {
            return;
        }

        let discord_config = &self.config.gateway.discord;

        // Check allowed channels
        if !discord_config.allowed_channels.is_empty() {
            let channel_id = msg.channel_id.get();
            if !discord_config.allowed_channels.contains(&channel_id) {
                return;
            }
        }

        let content = msg.content.clone();

        // Determine if we should respond:
        // - If require_mention is false: respond to ALL messages (free-form chat mode)
        // - If require_mention is true: only respond to mentions or prefix commands
        let bot_mentioned = msg.mentions_me(&ctx).await.unwrap_or(false);
        let has_prefix = !discord_config.command_prefix.is_empty()
            && content.starts_with(&discord_config.command_prefix);

        let should_respond = if discord_config.require_mention {
            // Legacy mode: need mention or prefix
            bot_mentioned || has_prefix
        } else {
            // Free-form mode: respond to everything (except our own messages, filtered above)
            true
        };

        if !should_respond {
            return;
        }

        // Strip prefix if present (for commands like !shark status)
        let clean_content = if has_prefix {
            content[discord_config.command_prefix.len()..].trim().to_string()
        } else {
            content.clone()
        };

        // Typing indicator
        if discord_config.typing_indicator {
            let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
        }

        let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<String>();

        let event = DiscordEvent::UserMessage {
            channel_id: msg.channel_id.get(),
            user_id: msg.author.id.get(),
            username: msg.author.name.clone(),
            content: clean_content,
            reply_tx,
        };

        if let Err(e) = self.event_tx.send(event) {
            error!("Failed to send Discord event: {}", e);
            return;
        }

        // Collect responses and send them back to Discord
        let mut full_response = String::new();
        while let Some(chunk) = reply_rx.recv().await {
            full_response.push_str(&chunk);
        }

        if !full_response.is_empty() {
            // Split if too long
            let max_len = discord_config.max_message_length;
            if full_response.len() > max_len {
                for chunk in full_response.chars().collect::<Vec<_>>().chunks(max_len) {
                    let chunk_str: String = chunk.iter().collect();
                    if let Err(e) = msg.channel_id.say(&ctx.http, &chunk_str).await {
                        error!("Failed to send Discord message: {}", e);
                    }
                }
            } else {
                if let Err(e) = msg.channel_id.say(&ctx.http, &full_response).await {
                    error!("Failed to send Discord message: {}", e);
                }
            }
        }
    }

    async fn interaction_create(&self, ctx: SerenityContext, interaction: Interaction) {
        if let Some(command) = interaction.as_command() {
            // Defer the response immediately for slow operations
            let deferred = CreateInteractionResponse::Defer(
                CreateInteractionResponseMessage::new(),
            );
            if let Err(e) = command.create_response(&ctx.http, deferred).await {
                error!("Failed to defer slash command: {}", e);
                return;
            }

            let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<String>();

            let event = DiscordEvent::SlashCommand {
                interaction: Interaction::Command(command.clone()),
                reply_tx,
            };

            if let Err(e) = self.event_tx.send(event) {
                error!("Failed to send slash command event: {}", e);
                return;
            }

            // Collect all response parts
            let mut full_response = String::new();
            while let Some(chunk) = reply_rx.recv().await {
                full_response.push_str(&chunk);
            }

            if !full_response.is_empty() {
                // Discord has a 2000 char limit for followup messages
                let max_len = self.config.gateway.discord.max_message_length;
                if full_response.len() > max_len {
                    // Send first chunk as followup, rest as additional followups
                    let chunks: Vec<String> = full_response
                        .chars()
                        .collect::<Vec<_>>()
                        .chunks(max_len)
                        .map(|c| c.iter().collect())
                        .collect();

                    for (i, chunk) in chunks.iter().enumerate() {
                        if i == 0 {
                            if let Err(e) = command
                                .create_followup(&ctx.http, CreateInteractionResponseFollowup::new().content(chunk))
                                .await
                            {
                                error!("Failed to send followup: {}", e);
                            }
                        } else {
                            if let Err(e) = command
                                .create_followup(&ctx.http, CreateInteractionResponseFollowup::new().content(chunk))
                                .await
                            {
                                error!("Failed to send followup chunk: {}", e);
                            }
                        }
                    }
                } else {
                    if let Err(e) = command
                        .create_followup(&ctx.http, CreateInteractionResponseFollowup::new().content(full_response))
                        .await
                    {
                        error!("Failed to send slash command followup: {}", e);
                    }
                }
            }
        }
    }

    async fn ready(&self, _ctx: SerenityContext, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);

        // Register slash commands
        let discord_config = &self.config.gateway.discord;
        let guild_ids = &discord_config.guild_ids;

        if guild_ids.is_empty() {
            // Global commands
            if let Err(e) = register_commands(&_ctx, None).await {
                warn!("Failed to register global commands: {}", e);
            }
        } else {
            for guild_id in guild_ids {
                if let Err(e) = register_commands(&_ctx, Some(GuildId::new(*guild_id))).await {
                    warn!("Failed to register commands for guild {}: {}", guild_id, e);
                }
            }
        }

        let _ = self.event_tx.send(DiscordEvent::Ready);
    }
}

/// Spawn the Discord bot as a background task.
pub fn spawn_bot(
    config: Config,
) -> mpsc::UnboundedReceiver<DiscordEvent> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    let bot = DiscordBot::new(config, event_tx);

    tokio::spawn(async move {
        if let Err(e) = bot.start().await {
            error!("Discord bot error: {}", e);
        }
    });

    event_rx
}
