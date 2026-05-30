//! Unified gateway abstraction — platform-agnostic event types.
//!
//! All gateways (Discord, Telegram, Slack, WhatsApp, Matrix) emit the same
//! `PlatformEvent` types, which the `MessageRouter` handles uniformly.

use tokio::sync::mpsc;

/// A user message received from any platform.
#[derive(Debug, Clone)]
pub struct UserMessage {
    pub platform: Platform,
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    pub content: String,
    /// Reply channel — send response strings here.
    pub reply_tx: mpsc::UnboundedSender<String>,
}

/// A platform-specific command/slash command.
#[derive(Debug, Clone)]
pub struct PlatformCommand {
    pub platform: Platform,
    pub command: String,
    pub args: String,
    pub channel_id: String,
    pub user_id: String,
    pub reply_tx: mpsc::UnboundedSender<String>,
}

/// Platform lifecycle events.
#[derive(Debug, Clone)]
pub enum PlatformEvent {
    UserMessage(UserMessage),
    Command(PlatformCommand),
    Ready { platform: Platform },
    Disconnected { platform: Platform },
}

/// Supported platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Discord,
    Telegram,
    Slack,
    WhatsApp,
    Matrix,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Discord => write!(f, "discord"),
            Platform::Telegram => write!(f, "telegram"),
            Platform::Slack => write!(f, "slack"),
            Platform::WhatsApp => write!(f, "whatsapp"),
            Platform::Matrix => write!(f, "matrix"),
        }
    }
}

/// Trait for platform gateways.
#[async_trait::async_trait]
pub trait Gateway: Send + Sync {
    /// Platform identifier.
    fn platform(&self) -> Platform;

    /// Start the gateway connection. Blocks until shutdown.
    async fn start(&self) -> anyhow::Result<()>;

    /// Send a message to a channel/user.
    async fn send_message(&self, channel_id: &str, content: &str) -> anyhow::Result<()>;
}
