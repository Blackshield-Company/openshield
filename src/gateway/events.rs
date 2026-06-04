//! Platform-neutral gateway events.
//!
//! All platform adapters normalize their events into this format,
//! allowing the MessageRouter to handle them uniformly.

use tokio::sync::mpsc;

/// Events emitted by any platform gateway.
#[derive(Debug, Clone)]
pub enum GatewayEvent {
    /// A user sent a message in a channel.
    UserMessage {
        channel_id: u64,
        user_id: u64,
        username: String,
        content: String,
        reply_tx: mpsc::UnboundedSender<String>,
    },
    /// Bot is ready.
    Ready,
    /// Bot disconnected.
    Disconnected,
}

/// Platform-specific slash/command interaction data.
/// This is a simplified representation; platforms that support
/// rich interactions handle them internally before emitting
/// a UserMessage or use platform-specific reply paths.
#[derive(Debug, Clone)]
#[allow(dead_code)]
#[allow(clippy::large_enum_variant)]
pub enum InteractionEvent {
    #[cfg(feature = "discord")]
    Discord {
        interaction: serenity::all::Interaction,
        reply_tx: mpsc::UnboundedSender<String>,
    },
    /// Placeholder for platforms without rich interactions.
    None,
}
