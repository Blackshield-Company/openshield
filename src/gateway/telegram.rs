//! Telegram adapter: Bot API long polling via teloxide.
//!
//! No webhook and no public server. Approvals use inline keyboard buttons.
//! Group chats are ignored when `dm_only` is true (the default).

use anyhow::{Context, Result};
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MaybeInaccessibleMessage, MessageKind,
};
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

/// Telegram Bot API adapter.
#[derive(Clone)]
pub struct TelegramAdapter {
    bot: Bot,
    dm_only: bool,
    allowed_chats: Vec<i64>,
    require_command_prefix: bool,
    max_inbound_bytes: usize,
}

impl TelegramAdapter {
    pub fn new(config: Config) -> Self {
        let tg = &config.gateway.telegram;
        let token = tg.bot_token.clone().unwrap_or_default();
        Self {
            bot: Bot::new(token),
            dm_only: tg.dm_only,
            allowed_chats: tg.allowed_chats.clone(),
            require_command_prefix: tg.require_command_prefix,
            max_inbound_bytes: tg.max_inbound_bytes.max(1),
        }
    }

    fn chat_accepted(&self, chat: &teloxide::types::Chat) -> bool {
        let is_private = chat.is_private();
        if self.dm_only && !is_private {
            return false;
        }
        if !is_private && self.allowed_chats.is_empty() {
            // Groups only when explicitly listed.
            return false;
        }
        if !self.allowed_chats.is_empty() && !self.allowed_chats.contains(&chat.id.0) && !is_private
        {
            return false;
        }
        true
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for TelegramAdapter {
    fn platform(&self) -> Platform {
        Platform::Telegram
    }

    async fn start(
        &self,
        tx: mpsc::Sender<Inbound>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        info!("Telegram adapter starting (long polling)");

        let adapter = self.clone();
        let tx_msg = tx.clone();
        let handler = dptree::entry()
            .branch(Update::filter_message().endpoint({
                let adapter = adapter.clone();
                move |bot: Bot, msg: Message| {
                    let adapter = adapter.clone();
                    let tx = tx_msg.clone();
                    async move {
                        handle_message(&adapter, &bot, msg, tx).await;
                        respond(())
                    }
                }
            }))
            .branch(Update::filter_callback_query().endpoint({
                let adapter = adapter.clone();
                let tx_cb = tx.clone();
                move |bot: Bot, q: CallbackQuery| {
                    let adapter = adapter.clone();
                    let tx = tx_cb.clone();
                    async move {
                        handle_callback(&adapter, &bot, q, tx).await;
                        respond(())
                    }
                }
            }));

        let mut dispatcher = Dispatcher::builder(self.bot.clone(), handler)
            .enable_ctrlc_handler()
            .build();

        tokio::select! {
            _ = dispatcher.dispatch() => {
                warn!("Telegram long poll ended");
            }
            _ = shutdown.changed() => {
                info!("Telegram adapter shutting down");
            }
        }
        Ok(())
    }

    async fn send(&self, dest: &Destination, msg: Outbound) -> Result<()> {
        let chat_id = dest.chat_id.parse::<i64>().map(ChatId).unwrap_or(ChatId(0));
        send_chunks(
            &self.bot,
            chat_id,
            dest.thread_id.as_deref(),
            msg.reply_to.as_deref(),
            &msg.text,
        )
        .await;
        Ok(())
    }

    async fn send_approval(
        &self,
        dest: &Destination,
        req: &ApprovalView,
    ) -> Result<ApprovalHandle> {
        let chat_id = dest
            .chat_id
            .parse::<i64>()
            .map(ChatId)
            .context("invalid telegram chat id")?;
        let text = format_approval(req);
        let keyboard = InlineKeyboardMarkup::new(vec![vec![
            InlineKeyboardButton::callback(
                "Approve",
                encode_approval_button(ButtonDecision::Approve, &req.id),
            ),
            InlineKeyboardButton::callback(
                "Deny",
                encode_approval_button(ButtonDecision::Reject, &req.id),
            ),
            InlineKeyboardButton::callback(
                "Always",
                encode_approval_button(ButtonDecision::ApproveAlways, &req.id),
            ),
        ]]);
        let mut reqst = self.bot.send_message(chat_id, text).reply_markup(keyboard);
        if let Some(thread) = &dest.thread_id
            && let Ok(id) = thread.parse::<i32>()
        {
            reqst =
                reqst.message_thread_id(teloxide::types::ThreadId(teloxide::types::MessageId(id)));
        }
        let sent = reqst.await.context("sending telegram approval card")?;
        Ok(ApprovalHandle {
            dest: dest.clone(),
            message_id: sent.id.0.to_string(),
        })
    }

    async fn edit_approval(&self, handle: &ApprovalHandle, req: &ApprovalView) -> Result<()> {
        let chat_id = handle
            .dest
            .chat_id
            .parse::<i64>()
            .map(ChatId)
            .context("invalid telegram chat id")?;
        let mid = teloxide::types::MessageId(
            handle
                .message_id
                .parse()
                .context("invalid telegram message id")?,
        );
        let text = format_approval(req);
        let _ = self
            .bot
            .edit_message_text(chat_id, mid, text)
            .reply_markup(InlineKeyboardMarkup::new(
                Vec::<Vec<InlineKeyboardButton>>::new(),
            ))
            .await;
        Ok(())
    }

    async fn delete_message(&self, dest: &Destination, message_id: &str) -> Result<()> {
        let chat_id = dest
            .chat_id
            .parse::<i64>()
            .map(ChatId)
            .context("invalid telegram chat id")?;
        let mid =
            teloxide::types::MessageId(message_id.parse().context("invalid telegram message id")?);
        let _ = self.bot.delete_message(chat_id, mid).await;
        Ok(())
    }
}

fn format_approval(req: &ApprovalView) -> String {
    if let Some(decision) = &req.decision {
        return format!("Approval {} — {decision}", req.id);
    }
    format!(
        "Approval needed\n\
         session: {}\n\
         tool: {}\n\
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

async fn send_chunks(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<&str>,
    reply_to: Option<&str>,
    text: &str,
) {
    const MAX_LEN: usize = 4096;
    let chunks: Vec<String> = if text.len() <= MAX_LEN {
        vec![text.to_string()]
    } else {
        text.chars()
            .collect::<Vec<_>>()
            .chunks(MAX_LEN)
            .map(|c| c.iter().collect())
            .collect()
    };
    for (i, chunk) in chunks.into_iter().enumerate() {
        let mut req = bot.send_message(chat_id, chunk);
        if let Some(thread) = thread_id
            && let Ok(id) = thread.parse::<i32>()
        {
            req = req.message_thread_id(teloxide::types::ThreadId(teloxide::types::MessageId(id)));
        }
        if i == 0
            && let Some(reply) = reply_to
            && let Ok(id) = reply.parse::<i32>()
        {
            req = req.reply_parameters(teloxide::types::ReplyParameters::new(
                teloxide::types::MessageId(id),
            ));
        }
        if let Err(e) = req.await {
            error!("Failed to send Telegram message: {e}");
        }
    }
}

async fn handle_message(
    adapter: &TelegramAdapter,
    _bot: &Bot,
    msg: Message,
    tx: mpsc::Sender<Inbound>,
) {
    if matches!(msg.kind, MessageKind::Empty {}) {
        return;
    }
    if !adapter.chat_accepted(&msg.chat) {
        return;
    }
    let is_dm = msg.chat.is_private();
    let text = msg
        .text()
        .or_else(|| msg.caption())
        .unwrap_or("")
        .to_string();

    if adapter.require_command_prefix
        && !text.starts_with('/')
        && msg.photo().is_none()
        && msg.document().is_none()
    {
        return;
    }

    let user = msg.from.clone();
    let username = user
        .as_ref()
        .map(|u| {
            u.username.clone().unwrap_or_else(|| {
                format!(
                    "{} {}",
                    u.first_name,
                    u.last_name.clone().unwrap_or_default()
                )
                .trim()
                .to_string()
            })
        })
        .unwrap_or_else(|| "Unknown".to_string());
    let user_id = user
        .map(|u| u.id.0.to_string())
        .unwrap_or_else(|| "0".into());

    let kind = if let Some(cmd) = parse_text_command(&text) {
        match cmd {
            BuiltinCommand::Pair { code } => InboundKind::Pair { code },
            other => InboundKind::Command(other),
        }
    } else if text.is_empty() && msg.photo().is_none() && msg.document().is_none() {
        return;
    } else {
        InboundKind::Message { text }
    };

    let dest = Destination::chat(msg.chat.id.0.to_string()).with_ids(
        msg.thread_id.map(|t| t.0.0.to_string()),
        Some(msg.id.0.to_string()),
    );

    let inbound = Inbound {
        platform: Platform::Telegram,
        kind,
        user_id,
        username,
        dest,
        attachments: Vec::new(), // media download is best-effort, added below
        raw_message_id: Some(msg.id.0.to_string()),
        is_dm,
    };

    // Best-effort attachment scan; skip if over the cap. We do not fail the
    // message if a download does not work (tests never hit this path).
    let inbound = match collect_attachments(adapter, _bot, &msg).await {
        Ok(atts) => Inbound {
            attachments: atts,
            ..inbound
        },
        Err(e) => {
            warn!("telegram attachment skipped: {e:#}");
            inbound
        }
    };

    if let Err(e) = tx.send(inbound).await {
        error!("Failed to enqueue Telegram event: {e}");
    }
}

async fn collect_attachments(
    adapter: &TelegramAdapter,
    bot: &Bot,
    msg: &Message,
) -> Result<Vec<crate::gateway::adapter::Attachment>> {
    let mut out = Vec::new();
    if let Some(photos) = msg.photo()
        && let Some(best) = photos.last()
        && (best.file.size as usize) <= adapter.max_inbound_bytes
        && let Some(att) = download_file(bot, &best.file.id.0, "photo.jpg", true).await?
    {
        out.push(att);
    }
    if let Some(doc) = msg.document()
        && (doc.file.size as usize) <= adapter.max_inbound_bytes
    {
        let name = doc
            .file_name
            .clone()
            .unwrap_or_else(|| "file.bin".to_string());
        let is_image = name.rsplit('.').next().is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp"
            )
        });
        if let Some(att) = download_file(bot, &doc.file.id.0, &name, is_image).await? {
            out.push(att);
        }
    }
    Ok(out)
}

async fn download_file(
    bot: &Bot,
    file_id: &str,
    name: &str,
    is_image: bool,
) -> Result<Option<crate::gateway::adapter::Attachment>> {
    let file = bot.get_file(file_id.to_owned().into()).await?;
    let dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield")
        .join("inbox");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "{}-{name}",
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    ));
    let mut dst = Vec::new();
    bot.download_file(&file.path, &mut dst).await?;
    std::fs::write(&path, &dst)?;
    Ok(Some(crate::gateway::adapter::Attachment {
        filename: name.to_string(),
        path,
        mime: None,
        is_image,
    }))
}

async fn handle_callback(
    adapter: &TelegramAdapter,
    bot: &Bot,
    q: CallbackQuery,
    tx: mpsc::Sender<Inbound>,
) {
    let Some(data) = q.data.clone() else {
        return;
    };
    let Some((decision, approval_id)) = parse_approval_button(&data) else {
        return;
    };
    let (chat, message_id, is_dm) = match &q.message {
        Some(MaybeInaccessibleMessage::Regular(msg)) => {
            if !adapter.chat_accepted(&msg.chat) {
                return;
            }
            (
                msg.chat.id.0.to_string(),
                Some(msg.id.0.to_string()),
                msg.chat.is_private(),
            )
        }
        _ => return,
    };
    // Clear the client-side spinner. Ignore errors (message may be old).
    let _ = bot.answer_callback_query(q.id).await;

    let user_id = q.from.id.0.to_string();
    let username = q
        .from
        .username
        .clone()
        .unwrap_or_else(|| q.from.first_name.clone());
    let inbound = Inbound {
        platform: Platform::Telegram,
        kind: InboundKind::ApprovalClick {
            approval_id,
            decision,
        },
        user_id,
        username,
        dest: Destination::chat(chat).with_ids(None, message_id.clone()),
        attachments: vec![],
        raw_message_id: message_id,
        is_dm,
    };
    if let Err(e) = tx.send(inbound).await {
        error!("Failed to enqueue Telegram callback: {e}");
    }
}

/// Kept so existing `spawn_bot` call sites in tests (if any) still type-check
/// during the transition. The supervisor is the only production host.
#[deprecated(note = "channels now run inside openshield serve")]
#[allow(dead_code)]
pub fn spawn_bot(config: Config) -> (mpsc::UnboundedReceiver<TelegramEvent>, TelegramReplySender) {
    let (tx, rx) = mpsc::unbounded_channel();
    let token = config
        .gateway
        .telegram
        .bot_token
        .clone()
        .unwrap_or_default();
    let bot = Bot::new(token);
    let reply_sender = TelegramReplySender { bot };
    drop(tx);
    (rx, reply_sender)
}

/// Legacy event type, kept so `unified_router` still compiles.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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

/// Legacy reply helper, kept so `unified_router` still compiles.
#[derive(Clone)]
#[allow(dead_code)]
pub struct TelegramReplySender {
    pub bot: Bot,
}

#[allow(dead_code)]
impl TelegramReplySender {
    pub async fn send_message(&self, chat_id: i64, text: &str) {
        send_chunks(&self.bot, ChatId(chat_id), None, None, text).await;
    }
}
