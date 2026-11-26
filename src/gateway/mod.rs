pub mod adapter;
pub mod allowlist;
#[allow(dead_code)]
pub mod channel_state;
#[cfg(feature = "discord")]
#[allow(dead_code)]
pub mod commands;
#[cfg(feature = "discord")]
pub mod discord;
#[allow(dead_code)]
pub mod events;
#[cfg(feature = "web-api")]
pub mod http;
#[allow(dead_code)]
pub mod matrix;
#[allow(dead_code)]
pub mod message_router;
pub mod parse;
pub mod platform;
pub mod session_branch;
pub mod session_map;
#[cfg(feature = "slack")]
#[allow(dead_code)]
pub mod slack;
#[cfg(feature = "web-api")]
pub mod supervisor;
#[cfg(feature = "telegram")]
pub mod telegram;
#[allow(dead_code)]
pub mod unified_router;

use serde::{Deserialize, Serialize};

/// Gateway configuration — replaces HermesIntegrationConfig.
#[derive(Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub discord: DiscordConfig,
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub slack: SlackConfig,
    #[serde(default)]
    pub whatsapp: WhatsAppConfig,
    #[serde(default)]
    pub matrix: MatrixConfig,
    #[serde(default)]
    pub mcp: McpGatewayConfig,
    /// Path to the persisted user-ID allowlist. Empty = default
    /// `~/.config/openshield/channels-allowlist.toml`.
    #[serde(default)]
    pub allowlist_path: String,
    /// When true, a successful `/pair` appends the user id to the allowlist file.
    #[serde(default = "default_true")]
    pub persist_paired_users: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: Option<String>,
    #[serde(default)]
    pub application_id: Option<String>,
    /// Guild IDs where slash commands should be registered (empty = global).
    #[serde(default)]
    pub guild_ids: Vec<u64>,
    /// Channel IDs the bot is allowed to respond in (empty = all).
    #[serde(default)]
    pub allowed_channels: Vec<u64>,
    /// Require @mention to trigger responses.
    /// Default: false — bot responds to all messages in allowed channels.
    #[serde(default)]
    pub require_mention: bool,
    /// Prefix for text commands (e.g., "!shield").
    /// Default: "!shield" — set to empty string to disable prefix commands.
    #[serde(default = "default_prefix")]
    pub command_prefix: String,
    /// Max message length before splitting.
    #[serde(default = "default_max_length")]
    pub max_message_length: usize,
    /// Enable typing indicator while generating.
    #[serde(default = "default_true")]
    pub typing_indicator: bool,
    /// Enable multi-model mode (query multiple models, compare responses).
    /// Default: false — single model responses for lower latency/cost.
    #[serde(default)]
    pub multi_model_enabled: bool,
    /// Secondary models for multi-model mode (when enabled).
    #[serde(default)]
    pub multi_model_secondary: Vec<String>,
    /// Discord user snowflakes allowed to talk. Empty + enabled = deny all except `/pair`.
    #[serde(default)]
    pub allowed_users: Vec<String>,
    /// `/shield` in a guild channel opens a thread that is the session.
    #[serde(default = "default_true")]
    pub thread_per_session: bool,
    /// Max inbound messages per user per 60 seconds.
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_min: usize,
    /// Max size in bytes of a downloaded attachment.
    #[serde(default = "default_discord_max_bytes")]
    pub max_inbound_bytes: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: Option<String>,
    /// Allowed chat IDs (empty = all).
    #[serde(default)]
    pub allowed_chats: Vec<i64>,
    /// Require commands to start with / (default: false — responds to all messages).
    #[serde(default)]
    pub require_command_prefix: bool,
    /// Telegram user ids allowed to talk. Empty + enabled = deny all except `/pair`.
    #[serde(default)]
    pub allowed_users: Vec<String>,
    /// Ignore group and supergroup chats (MVP default).
    #[serde(default = "default_true")]
    pub dm_only: bool,
    /// Max inbound messages per user per 60 seconds.
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_min: usize,
    /// Max size in bytes of a downloaded attachment.
    #[serde(default = "default_telegram_max_bytes")]
    pub max_inbound_bytes: usize,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct SlackConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Bot token (xoxb-...).
    #[serde(default)]
    pub bot_token: Option<String>,
    /// App-level token for Socket Mode (xapp-...).
    #[serde(default)]
    pub app_token: Option<String>,
    /// Allowed channel IDs (empty = all).
    #[serde(default)]
    pub allowed_channels: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct WhatsAppConfig {
    #[serde(default)]
    pub enabled: bool,
    /// For official Cloud API: permanent access token.
    #[serde(default)]
    pub access_token: Option<String>,
    /// Phone number ID for the WhatsApp Business account.
    #[serde(default)]
    pub phone_number_id: Option<String>,
    /// For unofficial (ruwa): session file path for QR login persistence.
    #[serde(default)]
    pub session_path: Option<String>,
    /// Use official Cloud API vs unofficial Web protocol.
    #[serde(default)]
    pub use_official_api: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct MatrixConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Homeserver URL (e.g., https://matrix.org).
    #[serde(default)]
    pub homeserver: Option<String>,
    /// User ID (e.g., @openshield:matrix.org).
    #[serde(default)]
    pub user_id: Option<String>,
    /// Access token.
    #[serde(default)]
    pub access_token: Option<String>,
    /// Allowed room IDs (empty = all).
    #[serde(default)]
    pub allowed_rooms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: std::collections::HashMap<String, String>,
    },
    Sse {
        url: String,
        headers: std::collections::HashMap<String, String>,
    },
}

fn default_true() -> bool {
    true
}
fn default_prefix() -> String {
    "!shield".to_string()
}
fn default_max_length() -> usize {
    2000
}
fn default_rate_limit() -> usize {
    20
}
fn default_telegram_max_bytes() -> usize {
    5_000_000
}
fn default_discord_max_bytes() -> usize {
    8_000_000
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            discord: DiscordConfig::default(),
            telegram: TelegramConfig::default(),
            slack: SlackConfig::default(),
            whatsapp: WhatsAppConfig::default(),
            matrix: MatrixConfig::default(),
            mcp: McpGatewayConfig::default(),
            allowlist_path: String::new(),
            persist_paired_users: true,
        }
    }
}

impl std::fmt::Debug for GatewayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayConfig")
            .field("discord", &self.discord)
            .field("telegram", &self.telegram)
            .field("slack", &self.slack)
            .field("whatsapp", &self.whatsapp)
            .field("matrix", &self.matrix)
            .field("mcp", &self.mcp)
            .field("allowlist_path", &self.allowlist_path)
            .field("persist_paired_users", &self.persist_paired_users)
            .finish()
    }
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: None,
            application_id: None,
            guild_ids: vec![],
            allowed_channels: vec![],
            require_mention: false,
            command_prefix: default_prefix(),
            max_message_length: default_max_length(),
            typing_indicator: true,
            multi_model_enabled: false,
            multi_model_secondary: vec![],
            allowed_users: vec![],
            thread_per_session: true,
            rate_limit_per_min: default_rate_limit(),
            max_inbound_bytes: default_discord_max_bytes(),
        }
    }
}

impl std::fmt::Debug for DiscordConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordConfig")
            .field("enabled", &self.enabled)
            .field("bot_token", &self.bot_token.as_ref().map(|_| "***"))
            .field("application_id", &self.application_id)
            .field("guild_ids", &self.guild_ids)
            .field("allowed_channels", &self.allowed_channels)
            .field("require_mention", &self.require_mention)
            .field("command_prefix", &self.command_prefix)
            .field("max_message_length", &self.max_message_length)
            .field("typing_indicator", &self.typing_indicator)
            .field("multi_model_enabled", &self.multi_model_enabled)
            .field("multi_model_secondary", &self.multi_model_secondary)
            .field("allowed_users", &self.allowed_users)
            .field("thread_per_session", &self.thread_per_session)
            .field("rate_limit_per_min", &self.rate_limit_per_min)
            .field("max_inbound_bytes", &self.max_inbound_bytes)
            .finish()
    }
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: None,
            allowed_chats: vec![],
            require_command_prefix: false,
            allowed_users: vec![],
            dm_only: true,
            rate_limit_per_min: default_rate_limit(),
            max_inbound_bytes: default_telegram_max_bytes(),
        }
    }
}

impl std::fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("enabled", &self.enabled)
            .field("bot_token", &self.bot_token.as_ref().map(|_| "***"))
            .field("allowed_chats", &self.allowed_chats)
            .field("require_command_prefix", &self.require_command_prefix)
            .field("allowed_users", &self.allowed_users)
            .field("dm_only", &self.dm_only)
            .field("rate_limit_per_min", &self.rate_limit_per_min)
            .field("max_inbound_bytes", &self.max_inbound_bytes)
            .finish()
    }
}

impl std::fmt::Debug for SlackConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlackConfig")
            .field("enabled", &self.enabled)
            .field("bot_token", &self.bot_token.as_ref().map(|_| "***"))
            .field("app_token", &self.app_token.as_ref().map(|_| "***"))
            .field("allowed_channels", &self.allowed_channels)
            .finish()
    }
}

impl std::fmt::Debug for WhatsAppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhatsAppConfig")
            .field("enabled", &self.enabled)
            .field("access_token", &self.access_token.as_ref().map(|_| "***"))
            .field("phone_number_id", &self.phone_number_id)
            .field("session_path", &self.session_path)
            .field("use_official_api", &self.use_official_api)
            .finish()
    }
}

impl std::fmt::Debug for MatrixConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixConfig")
            .field("enabled", &self.enabled)
            .field("homeserver", &self.homeserver)
            .field("user_id", &self.user_id)
            .field("access_token", &self.access_token.as_ref().map(|_| "***"))
            .field("allowed_rooms", &self.allowed_rooms)
            .finish()
    }
}

/// Replace a resolved secret with its `${ENV}` placeholder.
/// Production saves use `Config::to_toml_for_disk` instead; this helper
/// is kept for unit tests of the placeholder rewrite.
#[cfg(test)]
pub fn scrub_gateway_token(token: &mut Option<String>, env_name: &str) {
    let Some(value) = token.as_ref() else {
        return;
    };
    if value.starts_with("${") && value.ends_with('}') {
        return;
    }
    *token = Some(format!("${{{}}}", env_name));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_replaces_raw_token_keeps_placeholder() {
        let mut raw = Some("123:secret-value".to_string());
        scrub_gateway_token(&mut raw, "TELEGRAM_BOT_TOKEN");
        assert_eq!(raw.as_deref(), Some("${TELEGRAM_BOT_TOKEN}"));
        let mut placeholder = Some("${TELEGRAM_BOT_TOKEN}".to_string());
        scrub_gateway_token(&mut placeholder, "TELEGRAM_BOT_TOKEN");
        assert_eq!(placeholder.as_deref(), Some("${TELEGRAM_BOT_TOKEN}"));
        let mut empty = None;
        scrub_gateway_token(&mut empty, "TELEGRAM_BOT_TOKEN");
        assert!(empty.is_none());
    }

    #[test]
    fn debug_redacts_bot_tokens() {
        let tg = TelegramConfig {
            bot_token: Some("123:secret-value".into()),
            ..Default::default()
        };
        let d = format!("{:?}", tg);
        assert!(!d.contains("secret-value"));
        assert!(d.contains("***"));
        let dc = DiscordConfig {
            bot_token: Some("discord-secret-value".into()),
            ..Default::default()
        };
        let d = format!("{:?}", dc);
        assert!(!d.contains("discord-secret-value"));
    }
}
