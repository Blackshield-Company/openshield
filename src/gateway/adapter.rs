//! Platform-neutral channel adapter trait.
//!
//! Adapters only speak their network. Policy (allowlist, pairing, rate
//! limits, redaction, approvals, sessions) lives in the supervisor.

use anyhow::Result;
use tokio::sync::{mpsc, watch};

use crate::gateway::parse::ButtonDecision;
use crate::gateway::platform::Platform;

/// Where a reply or approval card should be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub chat_id: String,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

impl Destination {
    pub fn chat(chat_id: impl Into<String>) -> Self {
        Self {
            chat_id: chat_id.into(),
            thread_id: None,
            message_id: None,
        }
    }

    pub fn with_ids(mut self, thread_id: Option<String>, message_id: Option<String>) -> Self {
        self.thread_id = thread_id;
        self.message_id = message_id;
        self
    }
}

/// A file or image pulled off a chat message, already downloaded to disk.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub filename: String,
    pub path: std::path::PathBuf,
    pub mime: Option<String>,
    pub is_image: bool,
}

/// Outbound text (already redacted by the supervisor).
#[derive(Debug, Clone)]
pub struct Outbound {
    pub text: String,
    pub reply_to: Option<String>,
}

/// Pending or resolved approval card.
#[derive(Debug, Clone)]
pub struct ApprovalView {
    pub id: String,
    pub session: String,
    pub risk: String,
    pub tool: String,
    pub args: String,
    pub reason: String,
    pub expires: Option<String>,
    pub decision: Option<String>,
}

/// Handle returned by [`ChannelAdapter::send_approval`] so the card can be edited.
#[derive(Debug, Clone)]
pub struct ApprovalHandle {
    pub dest: Destination,
    pub message_id: String,
}

/// Kind of inbound event from a platform adapter.
#[derive(Debug, Clone)]
pub enum InboundKind {
    Message {
        text: String,
    },
    Command(crate::gateway::parse::BuiltinCommand),
    ApprovalClick {
        approval_id: String,
        decision: ButtonDecision,
    },
    Pair {
        code: String,
    },
}

/// An event from a platform, already normalized.
#[derive(Debug, Clone)]
pub struct Inbound {
    pub platform: Platform,
    pub kind: InboundKind,
    pub user_id: String,
    pub username: String,
    pub dest: Destination,
    pub attachments: Vec<Attachment>,
    /// Platform message id, used to delete a `/pair` message after success.
    pub raw_message_id: Option<String>,
    pub is_dm: bool,
}

/// A chat-platform adapter. Implementors must be cheap to clone (the
/// supervisor holds one copy for sending, `start` may hold another).
#[async_trait::async_trait]
pub trait ChannelAdapter: Send + Sync + 'static {
    fn platform(&self) -> Platform;

    /// Connect and run until `shutdown` is set or the connection ends.
    async fn start(&self, tx: mpsc::Sender<Inbound>, shutdown: watch::Receiver<bool>)
    -> Result<()>;

    async fn send(&self, dest: &Destination, msg: Outbound) -> Result<()>;

    async fn send_approval(&self, dest: &Destination, req: &ApprovalView)
    -> Result<ApprovalHandle>;

    async fn edit_approval(&self, handle: &ApprovalHandle, req: &ApprovalView) -> Result<()>;

    /// Best-effort deletion of a message (used to drop `/pair` codes).
    async fn delete_message(&self, dest: &Destination, message_id: &str) -> Result<()> {
        let _ = (dest, message_id);
        Ok(())
    }
}

/// Shared rate limiter: at most `per_min` events per (platform, user) per 60s.
pub struct RateLimiter {
    per_min: usize,
    hits: std::sync::Mutex<
        std::collections::HashMap<(String, String), std::collections::VecDeque<std::time::Instant>>,
    >,
}

impl RateLimiter {
    pub fn new(per_min: usize) -> Self {
        Self {
            per_min: per_min.max(1),
            hits: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Returns true if the event is allowed (and records it).
    pub fn allow(&self, platform: Platform, user_id: &str) -> bool {
        use std::time::{Duration, Instant};
        let key = (platform.to_string(), user_id.to_string());
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = Duration::from_secs(60);
        let entry = hits.entry(key).or_default();
        while entry
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= window)
        {
            entry.pop_front();
        }
        if entry.len() >= self.per_min {
            return false;
        }
        entry.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_caps() {
        let lim = RateLimiter::new(3);
        assert!(lim.allow(Platform::Telegram, "u"));
        assert!(lim.allow(Platform::Telegram, "u"));
        assert!(lim.allow(Platform::Telegram, "u"));
        assert!(!lim.allow(Platform::Telegram, "u"));
        // A different user is unaffected.
        assert!(lim.allow(Platform::Telegram, "other"));
        assert!(lim.allow(Platform::Discord, "u"));
    }
}
