//! Discord adapter: Gateway websocket via serenity.
//!
//! No public Interactions endpoint. `/shield` opens a session thread.
//! Approvals use message component buttons. DMs are used for `/pair`.

use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use serenity::all::{
    ButtonStyle, ChannelId, ChannelType, CommandInteraction, ComponentInteraction, CreateActionRow,
    CreateButton, CreateCommand, CreateCommandOption, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage, CreateMessage,
    CreateThread, EditMessage, GatewayIntents, Interaction, Ready,
};
use serenity::async_trait;
use serenity::client::{Context as SerenityContext, EventHandler};
use serenity::http::Http;
use serenity::model::channel::Message;
use serenity::model::id::GuildId;
use serenity::prelude::*;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::gateway::adapter::{
    ApprovalHandle, ApprovalView, ChannelAdapter, Destination, Inbound, InboundKind, Outbound,
};
use crate::gateway::parse::{
    BuiltinCommand, ButtonDecision, encode_approval_button, parse_approval_button,
    parse_text_command,
};
use crate::gateway::platform::Platform;

/// Shared HTTP client, set when the Gateway is ready.
type HttpSlot = Arc<OnceLock<Arc<Http>>>;

/// Discord Gateway adapter.
#[derive(Clone)]
pub struct DiscordAdapter {
    token: String,
    application_id: Option<String>,
    guild_ids: Vec<u64>,
    allowed_channels: Vec<u64>,
    thread_per_session: bool,
    max_message_length: usize,
    typing_indicator: bool,
    max_inbound_bytes: usize,
    http: HttpSlot,
}

impl DiscordAdapter {
    pub fn new(config: Config) -> Self {
        let dc = &config.gateway.discord;
        Self {
            token: dc.bot_token.clone().unwrap_or_default(),
            application_id: dc.application_id.clone(),
            guild_ids: dc.guild_ids.clone(),
            allowed_channels: dc.allowed_channels.clone(),
            thread_per_session: dc.thread_per_session,
            max_message_length: dc.max_message_length.max(1),
            typing_indicator: dc.typing_indicator,
            max_inbound_bytes: dc.max_inbound_bytes.max(1),
            http: Arc::new(OnceLock::new()),
        }
    }

    fn http(&self) -> Result<Arc<Http>> {
        self.http
            .get()
            .cloned()
            .context("Discord client is not ready yet")
    }

    fn channel_allowed(&self, channel_id: u64, is_dm: bool, is_our_thread: bool) -> bool {
        if is_dm || is_our_thread {
            return true;
        }
        if self.allowed_channels.is_empty() {
            return false;
        }
        self.allowed_channels.contains(&channel_id)
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for DiscordAdapter {
    fn platform(&self) -> Platform {
        Platform::Discord
    }

    async fn start(
        &self,
        tx: mpsc::Sender<Inbound>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        let intents = GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let handler = Handler {
            event_tx: tx,
            adapter: self.clone(),
        };

        let mut client = Client::builder(&self.token, intents)
            .event_handler(handler)
            .await
            .context("Failed to create Discord client")?;

        // Stash HTTP before start so send() works as soon as ready fires.
        let _ = self.http.set(client.http.clone());

        info!("Discord adapter starting (Gateway websocket)");
        tokio::select! {
            result = client.start() => {
                result.context("Discord client error")?;
            }
            _ = shutdown.changed() => {
                info!("Discord adapter shutting down");
            }
        }
        Ok(())
    }

    async fn send(&self, dest: &Destination, msg: Outbound) -> Result<()> {
        let http = self.http()?;
        let channel_id = dest
            .thread_id
            .as_ref()
            .or(Some(&dest.chat_id))
            .and_then(|s| s.parse::<u64>().ok())
            .map(ChannelId::new)
            .context("invalid discord channel id")?;
        for chunk in split_chunks(&msg.text, self.max_message_length) {
            channel_id
                .say(&http, &chunk)
                .await
                .context("sending Discord message")?;
        }
        Ok(())
    }

    async fn send_approval(
        &self,
        dest: &Destination,
        req: &ApprovalView,
    ) -> Result<ApprovalHandle> {
        let http = self.http()?;
        let channel_id = dest
            .thread_id
            .as_ref()
            .or(Some(&dest.chat_id))
            .and_then(|s| s.parse::<u64>().ok())
            .map(ChannelId::new)
            .context("invalid discord channel id")?;
        let row = CreateActionRow::Buttons(vec![
            CreateButton::new(encode_approval_button(ButtonDecision::Approve, &req.id))
                .label("Approve")
                .style(ButtonStyle::Success),
            CreateButton::new(encode_approval_button(ButtonDecision::Reject, &req.id))
                .label("Deny")
                .style(ButtonStyle::Danger),
            CreateButton::new(encode_approval_button(
                ButtonDecision::ApproveAlways,
                &req.id,
            ))
            .label("Always")
            .style(ButtonStyle::Secondary),
        ]);
        let sent = channel_id
            .send_message(
                &http,
                CreateMessage::new()
                    .content(format_approval(req))
                    .components(vec![row]),
            )
            .await
            .context("sending Discord approval card")?;
        Ok(ApprovalHandle {
            dest: dest.clone(),
            message_id: sent.id.get().to_string(),
        })
    }

    async fn edit_approval(&self, handle: &ApprovalHandle, req: &ApprovalView) -> Result<()> {
        let http = self.http()?;
        let channel_id = handle
            .dest
            .thread_id
            .as_ref()
            .or(Some(&handle.dest.chat_id))
            .and_then(|s| s.parse::<u64>().ok())
            .map(ChannelId::new)
            .context("invalid discord channel id")?;
        let mid = handle
            .message_id
            .parse::<u64>()
            .map(serenity::model::id::MessageId::new)
            .context("invalid discord message id")?;
        let _ = channel_id
            .edit_message(
                &http,
                mid,
                EditMessage::new()
                    .content(format_approval(req))
                    .components(Vec::new()),
            )
            .await;
        Ok(())
    }

    async fn delete_message(&self, dest: &Destination, message_id: &str) -> Result<()> {
        let http = self.http()?;
        let channel_id = dest
            .chat_id
            .parse::<u64>()
            .map(ChannelId::new)
            .context("invalid discord channel id")?;
        let mid = message_id
            .parse::<u64>()
            .map(serenity::model::id::MessageId::new)
            .context("invalid discord message id")?;
        let _ = http.delete_message(channel_id, mid, None).await;
        Ok(())
    }
}

fn format_approval(req: &ApprovalView) -> String {
    if let Some(decision) = &req.decision {
        return format!("Approval {} — {decision}", req.id);
    }
    format!(
        "**Approval needed**\n\
         session: `{}`\n\
         tool: `{}`\n\
         risk: {}\n\
         args: {}\n\
         {}\n\
         {}",
        req.session,
        req.tool,
        req.risk,
        req.args,
        req.reason,
        req.expires
            .as_deref()
            .map(|e| format!("expires {e}"))
            .unwrap_or_default()
    )
}

fn split_chunks(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    text.chars()
        .collect::<Vec<_>>()
        .chunks(max_len)
        .map(|c| c.iter().collect())
        .collect()
}

struct Handler {
    event_tx: mpsc::Sender<Inbound>,
    adapter: DiscordAdapter,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: SerenityContext, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);
        let _ = self.adapter.http.set(ctx.http.clone());
        if let Err(e) = register_slash_commands(&ctx, &self.adapter).await {
            warn!("Failed to register Discord slash commands: {e:#}");
        }
    }

    async fn message(&self, ctx: SerenityContext, msg: Message) {
        if msg.author.bot {
            return;
        }
        let is_dm = msg.guild_id.is_none();
        let channel_id = msg.channel_id.get();
        // A thread we already have a session for is accepted; the supervisor
        // still enforces the user allowlist. Unknown guild channels are dropped
        // unless they are on allowed_channels (and only then for /shield).
        let in_thread = msg.thread.as_ref().is_some()
            || ctx
                .http
                .get_channel(msg.channel_id)
                .await
                .ok()
                .and_then(|c| c.guild())
                .is_some_and(|c| {
                    matches!(
                        c.kind,
                        ChannelType::PublicThread
                            | ChannelType::PrivateThread
                            | ChannelType::NewsThread
                    )
                });
        if !self.adapter.channel_allowed(channel_id, is_dm, in_thread) {
            return;
        }
        // In a parent allowlisted channel (not a thread, not a DM), ignore
        // free-form chat — users should use /shield to open a session.
        if !is_dm && !in_thread {
            return;
        }
        if self.adapter.typing_indicator {
            let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
        }

        let text = msg.content.clone();
        let kind = if let Some(cmd) = parse_text_command(&text) {
            match cmd {
                BuiltinCommand::Pair { code } => InboundKind::Pair { code },
                other => InboundKind::Command(other),
            }
        } else {
            InboundKind::Message { text }
        };

        let dest = Destination::chat(channel_id.to_string()).with_ids(
            if in_thread {
                Some(channel_id.to_string())
            } else {
                None
            },
            Some(msg.id.get().to_string()),
        );

        let inbound = Inbound {
            platform: Platform::Discord,
            kind,
            user_id: msg.author.id.get().to_string(),
            username: msg.author.name.clone(),
            dest,
            attachments: Vec::new(),
            raw_message_id: Some(msg.id.get().to_string()),
            is_dm,
        };

        if let Err(e) = self.event_tx.send(inbound).await {
            error!("Failed to enqueue Discord message: {e}");
        }
        let _ = (self.adapter.max_inbound_bytes, ctx);
    }

    async fn interaction_create(&self, ctx: SerenityContext, interaction: Interaction) {
        if let Some(component) = interaction.as_message_component() {
            handle_component(&self.event_tx, &ctx, component).await;
            return;
        }
        if let Some(command) = interaction.as_command() {
            handle_slash(&self.event_tx, &self.adapter, &ctx, command).await;
        }
    }
}

async fn handle_component(
    tx: &mpsc::Sender<Inbound>,
    ctx: &SerenityContext,
    component: &ComponentInteraction,
) {
    let Some((decision, approval_id)) = parse_approval_button(&component.data.custom_id) else {
        return;
    };
    let ack = CreateInteractionResponse::Acknowledge;
    if let Err(e) = component.create_response(&ctx.http, ack).await {
        error!("Failed to ack Discord button: {e}");
        return;
    }
    let is_dm = component.guild_id.is_none();
    let channel_id = component.channel_id.get();
    let inbound = Inbound {
        platform: Platform::Discord,
        kind: InboundKind::ApprovalClick {
            approval_id,
            decision,
        },
        user_id: component.user.id.get().to_string(),
        username: component.user.name.clone(),
        dest: Destination::chat(channel_id.to_string()).with_ids(
            Some(channel_id.to_string()),
            Some(component.message.id.get().to_string()),
        ),
        attachments: vec![],
        raw_message_id: Some(component.message.id.get().to_string()),
        is_dm,
    };
    if let Err(e) = tx.send(inbound).await {
        error!("Failed to enqueue Discord button: {e}");
    }
}

async fn handle_slash(
    tx: &mpsc::Sender<Inbound>,
    adapter: &DiscordAdapter,
    ctx: &SerenityContext,
    command: &CommandInteraction,
) {
    let deferred = CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new());
    if let Err(e) = command.create_response(&ctx.http, deferred).await {
        error!("Failed to defer Discord slash command: {e}");
        return;
    }

    let name = command.data.name.as_str();
    let options = &command.data.options;
    let arg = |key: &str| -> String {
        options
            .iter()
            .find(|o| o.name == key)
            .and_then(|o| o.value.as_str())
            .unwrap_or("")
            .to_string()
    };

    let is_dm = command.guild_id.is_none();
    let parent_id = command.channel_id.get();

    // /pair is DM only — the supervisor also enforces this, but reject early.
    if name == "pair" && !is_dm {
        let _ = command
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .content("Pairing is only allowed in a direct message.")
                    .ephemeral(true),
            )
            .await;
        return;
    }

    // /shield in a guild channel opens a thread that becomes the session.
    let (chat_id, thread_id) = if name == "shield" && !is_dm && adapter.thread_per_session {
        match open_session_thread(ctx, command).await {
            Ok(tid) => (parent_id.to_string(), Some(tid)),
            Err(e) => {
                let _ = command
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!("Could not open a session thread: {e:#}")),
                    )
                    .await;
                return;
            }
        }
    } else {
        (parent_id.to_string(), None)
    };

    let kind = match name {
        "pair" => InboundKind::Pair { code: arg("code") },
        "new" => InboundKind::Command(BuiltinCommand::New),
        "status" => InboundKind::Command(BuiltinCommand::Status),
        "help" => InboundKind::Command(BuiltinCommand::Help),
        "shield" | "chat" => {
            let message = arg("message");
            if message.is_empty() {
                InboundKind::Command(BuiltinCommand::Shield { message: None })
            } else {
                InboundKind::Command(BuiltinCommand::Shield {
                    message: Some(message),
                })
            }
        }
        _ => {
            let _ = command
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new().content("Unknown command."),
                )
                .await;
            return;
        }
    };

    let inbound = Inbound {
        platform: Platform::Discord,
        kind,
        user_id: command.user.id.get().to_string(),
        username: command.user.name.clone(),
        dest: Destination::chat(chat_id).with_ids(thread_id, None),
        attachments: vec![],
        raw_message_id: None,
        is_dm,
    };
    if let Err(e) = tx.send(inbound).await {
        error!("Failed to enqueue Discord slash command: {e}");
    }
    // The supervisor will send the real reply in the thread / DM.
    let _ = command
        .create_followup(
            &ctx.http,
            CreateInteractionResponseFollowup::new().content("Working…"),
        )
        .await;
}

async fn open_session_thread(
    ctx: &SerenityContext,
    command: &CommandInteraction,
) -> Result<String> {
    let name = format!(
        "openshield-{}",
        command.user.name.chars().take(16).collect::<String>()
    );
    let builder = CreateThread::new(name).kind(ChannelType::PrivateThread);
    match command.channel_id.create_thread(&ctx.http, builder).await {
        Ok(thread) => Ok(thread.id.get().to_string()),
        Err(_) => {
            let public = CreateThread::new("openshield").kind(ChannelType::PublicThread);
            let thread = command
                .channel_id
                .create_thread(&ctx.http, public)
                .await
                .context("creating public session thread")?;
            Ok(thread.id.get().to_string())
        }
    }
}

async fn register_slash_commands(ctx: &SerenityContext, adapter: &DiscordAdapter) -> Result<()> {
    let commands = vec![
        CreateCommand::new("shield")
            .description("Start or continue an OpenShield session")
            .add_option(
                CreateCommandOption::new(
                    serenity::all::CommandOptionType::String,
                    "message",
                    "Your message",
                )
                .required(false),
            ),
        CreateCommand::new("pair")
            .description("Pair this Discord account with a one-time code (DM only)")
            .add_option(
                CreateCommandOption::new(
                    serenity::all::CommandOptionType::String,
                    "code",
                    "Pairing code from `openshield pair`",
                )
                .required(true),
            ),
        CreateCommand::new("new").description("Start a fresh OpenShield session"),
        CreateCommand::new("status").description("Show the current session and model"),
        CreateCommand::new("help").description("Show OpenShield channel commands"),
    ];
    if adapter.guild_ids.is_empty() {
        serenity::all::Command::set_global_commands(&ctx.http, commands)
            .await
            .context("registering global commands")?;
    } else {
        for guild_id in &adapter.guild_ids {
            GuildId::new(*guild_id)
                .set_commands(&ctx.http, commands.clone())
                .await
                .with_context(|| format!("registering commands for guild {guild_id}"))?;
        }
    }
    let _ = &adapter.application_id;
    Ok(())
}

fn option_str<'a>(options: &'a [serenity::all::CommandDataOption], name: &str) -> Option<&'a str> {
    options
        .iter()
        .find(|o| o.name == name)
        .and_then(|o| o.value.as_str())
}

/// Legacy event type, kept so `unified_router` still compiles.
#[derive(Debug, Clone)]
#[allow(dead_code, clippy::large_enum_variant)]
pub enum DiscordEvent {
    UserMessage {
        channel_id: u64,
        user_id: u64,
        username: String,
        content: String,
        reply_tx: mpsc::UnboundedSender<String>,
    },
    SlashCommand {
        interaction: Interaction,
        reply_tx: mpsc::UnboundedSender<String>,
    },
    Ready,
    Disconnected,
}

/// Legacy host, unused. Channels now run inside `openshield serve`.
#[deprecated(note = "channels now run inside openshield serve")]
#[allow(dead_code)]
pub fn spawn_bot(_config: Config) -> mpsc::UnboundedReceiver<DiscordEvent> {
    let (_tx, rx) = mpsc::unbounded_channel();
    rx
}

#[allow(dead_code)]
fn unused_helpers() {
    let _ = option_str;
}
