pub mod discord;
pub mod message_router;
pub mod commands;

use serde::{Deserialize, Serialize};

/// Gateway configuration — replaces HermesIntegrationConfig.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayConfig {
    #[serde(default)]
    pub discord: DiscordConfig,
    #[serde(default)]
    pub telegram: TelegramConfig,
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
    #[serde(default = "default_true")]
    pub require_mention: bool,
    /// Prefix for text commands (e.g., "!shark").
    #[serde(default = "default_prefix")]
    pub command_prefix: String,
    /// Max message length before splitting.
    #[serde(default = "default_max_length")]
    pub max_message_length: usize,
    /// Enable typing indicator while generating.
    #[serde(default = "default_true")]
    pub typing_indicator: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: Option<String>,
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
    Stdio { command: String, args: Vec<String>, env: std::collections::HashMap<String, String> },
    Sse { url: String, headers: std::collections::HashMap<String, String> },
}

fn default_true() -> bool { true }
fn default_prefix() -> String { "!shark".to_string() }
fn default_max_length() -> usize { 2000 }
