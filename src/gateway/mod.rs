#[cfg(any(
    feature = "discord",
    feature = "telegram",
    feature = "slack",
    feature = "matrix"
))]
pub mod channel_state;
#[cfg(feature = "discord")]
pub mod commands;
#[cfg(feature = "discord")]
pub mod discord;
#[cfg(any(
    feature = "discord",
    feature = "telegram",
    feature = "slack",
    feature = "matrix"
))]
pub mod events;
#[cfg(feature = "web-api")]
pub mod http;
#[cfg(feature = "matrix")]
pub mod matrix;
#[cfg(any(
    feature = "discord",
    feature = "telegram",
    feature = "slack",
    feature = "matrix"
))]
pub mod message_router;
#[cfg(any(
    feature = "discord",
    feature = "telegram",
    feature = "slack",
    feature = "matrix"
))]
pub mod platform;
#[cfg(any(
    feature = "discord",
    feature = "telegram",
    feature = "slack",
    feature = "matrix"
))]
pub mod session_branch;
#[cfg(feature = "slack")]
pub mod slack;
#[cfg(feature = "telegram")]
pub mod telegram;
#[cfg(any(
    feature = "discord",
    feature = "telegram",
    feature = "slack",
    feature = "matrix"
))]
pub mod unified_router;

use serde::{Deserialize, Serialize};

/// Gateway configuration — replaces HermesIntegrationConfig.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

pub fn missing_feature_warning(
    platform: &str,
    enabled: bool,
    feature_compiled: bool,
) -> Option<String> {
    if enabled && !feature_compiled {
        Some(format!(
            "{platform} gateway enabled in config but this build lacks the {platform} feature; rebuild with --features {platform}"
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_configs_default_disabled_when_parsing_empty_config() {
        let gateway: GatewayConfig = toml::from_str("").expect("empty config");
        assert!(!gateway.discord.enabled);
        assert!(!gateway.telegram.enabled);
        assert!(!gateway.slack.enabled);
        assert!(!gateway.matrix.enabled);

        let config: crate::config::Config = toml::from_str(
            "version = \"0\"\ndefault_model = \"m\"\nmemory_db_path = \"memory.db\"\ntools_enabled = []\nauto_route = false\ncost_limit_usd = 0.0\n\n[providers]\n",
        )
        .expect("config without gateway section");
        assert!(!config.gateway.discord.enabled);
        assert!(!config.gateway.telegram.enabled);
        assert!(!config.gateway.slack.enabled);
        assert!(!config.gateway.matrix.enabled);
    }

    #[test]
    fn missing_feature_warning_when_enabled_but_feature_missing() {
        assert_eq!(
            missing_feature_warning("discord", true, false).as_deref(),
            Some(
                "discord gateway enabled in config but this build lacks the discord feature; rebuild with --features discord"
            )
        );
        assert!(missing_feature_warning("discord", false, false).is_none());
        assert!(missing_feature_warning("discord", true, true).is_none());
        assert!(missing_feature_warning("telegram", false, true).is_none());
    }
}
