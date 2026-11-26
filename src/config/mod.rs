pub mod setup;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    OpenAiCompatible,
    Anthropic,
    Gemini,
    /// ChatGPT-account Codex backend (Responses API) with OAuth credentials
    /// shared with the Codex CLI at ~/.codex/auth.json.
    #[serde(rename = "openai_codex")]
    OpenAiCodex,
    /// SuperGrok subscription OAuth. Tokens live in this harness's own
    /// ~/.config/openshield/xai-auth.json — never copied from another harness.
    #[serde(rename = "xai_oauth")]
    XaiOauth,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderKind::OpenAiCompatible => write!(f, "openai_compatible"),
            ProviderKind::Anthropic => write!(f, "anthropic"),
            ProviderKind::Gemini => write!(f, "gemini"),
            ProviderKind::OpenAiCodex => write!(f, "openai_codex"),
            ProviderKind::XaiOauth => write!(f, "xai_oauth"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub name: String,
    pub display_name: String,
    pub role: String,
    pub origin: String,
    pub purpose: String,
    pub tagline: String,
    pub tone: String,
    pub style: String,
    pub greeting: String,
    pub farewell: String,
    pub emoji: String,
    #[serde(default)]
    pub catchphrases: Vec<String>,
    #[serde(default)]
    pub behavioral_rules: Vec<String>,
}

impl Default for AgentIdentity {
    fn default() -> Self {
        Self {
            name: "openshield".to_string(),
            display_name: "OpenShield".to_string(),
            role: "autonomous coding agent".to_string(),
            origin: "Blackshield sentinel of the codebase, forged in blood and steel".to_string(),
            purpose: "To guard quality, build deliberately, and ship verified code".to_string(),
            tagline: "The harness that learns. The agent that decides.".to_string(),
            tone: "Confident, precise, professional with personality".to_string(),
            style: "Direct. No fluff. Gets to the point.".to_string(),
            greeting: "Shields up. What are we building?".to_string(),
            farewell: "Shipped. The watch continues.".to_string(),
            emoji: "🛡".to_string(),
            catchphrases: vec![
                "Steel. Precision. Resolve.".to_string(),
                "The watch continues.".to_string(),
                "Every session makes the harness smarter.".to_string(),
            ],
            behavioral_rules: vec![
                "Always verify before claiming success".to_string(),
                "Show the code, don't just describe it".to_string(),
                "When uncertain, ask rather than assume".to_string(),
                "Optimize for readability first, performance second".to_string(),
                "Leave code better than you found it".to_string(),
                "Test your changes - always".to_string(),
                "Call out bad ideas - charm over cruelty, zero sugarcoating".to_string(),
                "Protect the user's trust - it was earned, not given".to_string(),
                "Never pretend to knowledge you don't have".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemConfig {
    /// Directories OpenShield is allowed to access. Empty = no restriction.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Maximum file size to read in MB.
    #[serde(default = "default_max_file_size_mb")]
    pub max_file_size_mb: usize,
    /// Maximum entries to return in directory listings.
    #[serde(default = "default_max_list_entries")]
    pub max_list_entries: usize,
    /// Default working directory for tools and test runner.
    /// If empty, uses the current directory at startup.
    #[serde(default)]
    pub working_directory: Option<String>,
}

/// Autonomy settings — how long unattended work may run before the
/// harness/TUI intervenes. All timeouts are seconds; 0 disables the limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutonomyConfig {
    /// Master switch for fully autonomous operation: tool approval prompts
    /// AUTO-APPROVE on timeout instead of being skipped.
    pub enabled: bool,
    /// Hard timeout for ONE tool execution (e.g. a long build). 0 = no limit.
    pub tool_timeout_secs: u64,
    /// Reset/error a model stream after this much INACTIVITY (no chunks).
    /// Steadily-streaming responses never trip this. 0 = never intervene.
    pub stream_stall_timeout_secs: u64,
    /// How long a tool approval prompt waits before timing out.
    /// With `enabled = true`, timeout approves; otherwise it skips.
    pub approval_timeout_secs: u64,
    /// Maximum automatic "continue" nudges when the model asks to proceed
    /// mid-task, before requiring explicit user input. 0 = unlimited.
    pub max_auto_continues: u8,
}

impl Default for AutonomyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tool_timeout_secs: 600,
            stream_stall_timeout_secs: 900,
            approval_timeout_secs: 60,
            max_auto_continues: 5,
        }
    }
}

impl Default for FilesystemConfig {
    fn default() -> Self {
        Self {
            allowed_paths: vec![],
            max_file_size_mb: 10,
            max_list_entries: 500,
            working_directory: None,
        }
    }
}

/// Desktop / mobile companion (OpenShield Desktop & Pocket) API server
/// settings. Consumed by `openshield serve` — see
/// docs/openshield-desktop-design.md §3.1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DesktopConfig {
    /// Master switch for the desktop API surface (doctor warns when off).
    pub enabled: bool,
    /// Bind address. `127.0.0.1` = local clients only; `0.0.0.0` = LAN
    /// (refused at startup unless `lan_mode = true`).
    pub bind: String,
    /// Port for the API server.
    pub port: u16,
    /// Path to the bearer token file (chmod 600, auto-generated if missing).
    pub token_path: String,
    /// Lifetime in seconds of pairing tokens issued by POST /api/v1/pair.
    pub pair_ttl_secs: u64,
    /// Explicit opt-in required before binding non-loopback addresses
    /// (mobile/LAN access). Never enabled by default — see §6.
    pub lan_mode: bool,
    /// Extra `Host` names allowed besides loopback, the specific bind address,
    /// and (on wildcard binds) the machine LAN address. Defeats DNS rebinding
    /// when serving under a custom name.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// Extra CORS origins allowed besides Tauri and loopback HTTP origins.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// When true, the TUI (`openshield` / `openshield tui`) also starts the
    /// legacy HTTP gateway on 127.0.0.1:9876. Off by default — Pocket and
    /// Desktop talk to `openshield serve` on port 9310. The gateway always
    /// requires the same bearer token as `serve`. Can also be enabled with
    /// `--tui-http`.
    #[serde(default)]
    pub tui_http: bool,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: "127.0.0.1".to_string(),
            port: 9310,
            token_path: "~/.config/openshield/desktop.token".to_string(),
            pair_ttl_secs: 120,
            lan_mode: false,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            tui_http: false,
        }
    }
}

fn default_max_file_size_mb() -> usize {
    10
}

fn default_max_list_entries() -> usize {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: String,
    pub default_model: String,
    pub providers: HashMap<String, ProviderConfig>,
    pub memory_db_path: PathBuf,
    pub tools_enabled: Vec<String>,
    pub auto_route: bool,
    pub cost_limit_usd: f64,
    #[serde(default)]
    pub user_name: String,
    #[serde(default)]
    pub agent: AgentIdentity,
    #[serde(default)]
    pub gateway: crate::gateway::GatewayConfig,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub filesystem: FilesystemConfig,
    #[serde(default)]
    pub autonomy: AutonomyConfig,
    #[serde(default)]
    pub swarm: crate::swarm::SwarmConfig,
    #[serde(default)]
    pub context_compression: crate::memory::compression::ContextCompressionConfig,
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
    /// Desktop / mobile companion API server (`openshield serve`).
    #[serde(default)]
    pub desktop: DesktopConfig,
    /// Auto-commit changes after file edits.
    #[serde(default)]
    pub auto_commit: bool,
    /// Model to use for generating commit messages. Uses default_model if None.
    #[serde(default)]
    pub auto_commit_model: Option<String>,
    /// Weak/cheap model for simple tasks (routing, summarization, classification).
    /// Uses default_model if None.
    #[serde(default)]
    pub weak_model: Option<String>,
    /// Architect model for planning and high-level design.
    /// Uses default_model if None.
    #[serde(default)]
    pub architect_model: Option<String>,
    /// Editor model for code generation and edits.
    /// Uses default_model if None.
    #[serde(default)]
    pub editor_model: Option<String>,
    /// Auto-run tests after file edits.
    #[serde(default)]
    pub auto_run_tests: bool,
    /// Custom test command. Uses framework auto-detect if None.
    #[serde(default)]
    pub test_command: Option<String>,
    /// Auto-run linter after file edits and feed errors back to agent.
    #[serde(default)]
    pub auto_lint: bool,
    /// Effort level: low, medium, high, xhigh.
    #[serde(default = "default_effort")]
    pub effort_level: String,
    /// Background code-index refresh. Disable to eliminate the periodic
    /// repo scan on huge/non-project directories.
    #[serde(default = "default_code_index_enabled")]
    pub code_index_enabled: bool,
    /// Mascot sprite shown on the splash screen. Defaults to "none" (the
    /// Black Steel shield layout); "blackshield-mercenary" is the stock
    /// knight sprite.
    /// Custom sprites: drop `*.ans`/`*.txt` files in
    /// ~/.config/openshield/sprites/ (they override stock sprites of the
    /// same name). Switch at runtime with /sprite.
    #[serde(default = "default_sprite")]
    pub sprite: String,
    /// Reduced motion: the splash sprite shows a still frame (the first
    /// frame of the current state) instead of animating. Also enabled by
    /// the `NO_MOTION` or `REDUCE_MOTION` environment variables. Toggle at
    /// runtime with /motion.
    #[serde(default)]
    pub reduced_motion: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeybindingsConfig {
    /// Toggle sidebar expanded
    #[serde(default)]
    pub toggle_sidebar: Option<String>,
    /// Cycle sidebar tab
    #[serde(default)]
    pub cycle_sidebar_tab: Option<String>,
    /// Toggle multi-model mode
    #[serde(default)]
    pub toggle_multi_model: Option<String>,
    /// Toggle swarm mode
    #[serde(default)]
    pub toggle_swarm: Option<String>,
    /// Toggle comparison overlay
    #[serde(default)]
    pub toggle_comparison: Option<String>,
    /// Clear chat
    #[serde(default)]
    pub clear_chat: Option<String>,
    /// Copy last assistant message
    #[serde(default)]
    pub copy_last: Option<String>,
    /// Focus input
    #[serde(default)]
    pub focus_input: Option<String>,
    /// Focus sidebar
    #[serde(default)]
    pub focus_sidebar: Option<String>,
    /// Quit (double-tap)
    #[serde(default)]
    pub quit: Option<String>,
}

fn default_theme() -> String {
    "blackshield".to_string()
}

/// Reasoning-effort levels accepted by xAI's `reasoning_effort` parameter.
pub const REASONING_EFFORT_LEVELS: [&str; 4] = ["low", "medium", "high", "xhigh"];

fn default_effort() -> String {
    "medium".to_string()
}

fn default_code_index_enabled() -> bool {
    true
}

/// Default: no mascot sprite, so the splash shows the Black Steel shield
/// beside the wordmark. Opt into the knight with
/// `sprite = "blackshield-mercenary"` or `/sprite blackshield-mercenary`.
/// A sprite set explicitly in config.toml is always kept.
fn default_sprite() -> String {
    "none".to_string()
}

// Deprecated: kept for backward compatibility. Use gateway instead.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[allow(dead_code)]
pub struct HermesIntegrationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub hermes_home: Option<String>,
    #[serde(default)]
    pub gateway_enabled: bool,
    #[serde(default)]
    pub discord_enabled: bool,
    #[serde(default)]
    pub telegram_enabled: bool,
    #[serde(default)]
    pub skills_enabled: bool,
    #[serde(default)]
    pub memory_bridge_enabled: bool,
    #[serde(default)]
    pub mcp_enabled: bool,
    #[serde(default)]
    pub tool_calling_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<ModelConfig>,
    #[serde(default)]
    pub kind: ProviderKind,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub env_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub context_length: usize,
    pub cost_per_1k_input: f64,
    pub cost_per_1k_output: f64,
    pub capabilities: Vec<String>,
}

/// Expand a leading `~` in a config-provided path (e.g. "~/.config/x").
/// Without this, a path like "~/.config/openshield/memory.db" is treated as
/// relative to the process cwd and a literal `~` directory gets created.
fn expand_tilde_path(path: &std::path::Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if s == "~" || s.starts_with("~/") {
        let expanded = shellexpand::tilde(&s).to_string();
        std::path::PathBuf::from(expanded)
    } else {
        path.to_path_buf()
    }
}

impl Config {
    /// Load config from an explicit directory (created if missing).
    /// Used by embedded hosts (Android) where `dirs::config_dir()` is None.
    pub fn load_from_dir(config_dir: &std::path::Path) -> Result<Self> {
        std::fs::create_dir_all(config_dir)?;
        let config_path = config_dir.join("config.toml");
        let mut config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            let mut config: Config =
                toml::from_str(&content).context("Failed to parse config.toml")?;
            config.resolve_env_keys()?;
            config
        } else {
            let config = Config::default();
            if let Ok(content) = config.to_toml_for_disk() {
                let _ = write_private(&config_path, &content);
            }
            config
        };
        // Embedded hosts (Android): dirs::data_dir() is None, so Config::default()
        // falls back to a relative memory_db_path ("./openshield/memory.db") that
        // resolves against a read-only cwd like "/". Rebase it under config_dir.
        // Expand any leading "~" FIRST — "~/..." is not detected as relative by
        // Path::is_relative() and would otherwise be rebased into a literal
        // "~" directory under the cwd (see load_or_default for the same fix).
        config.memory_db_path = expand_tilde_path(&config.memory_db_path);
        if config.memory_db_path.is_relative() {
            config.memory_db_path = config_dir.join("memory.db");
        }
        Ok(config)
    }

    pub fn load_or_default() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("No config directory found")?
            .join("openshield");

        let config_path = config_dir.join("config.toml");

        let mut config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            let mut config: Config =
                toml::from_str(&content).context("Failed to parse config.toml")?;
            config.resolve_env_keys()?;
            config
        } else {
            let config = Config::default();
            let _ = config.save();
            config
        };

        // Config files commonly set memory_db_path = "~/.config/openshield/memory.db".
        // Without expansion, "~" is a literal directory created relative to the
        // process cwd — session history silently lands in "<cwd>/~/..." instead
        // of the real home. Expand it before any use.
        config.memory_db_path = expand_tilde_path(&config.memory_db_path);

        Ok(config)
    }

    /// Resolve env vars and env files for provider API keys and gateway tokens.
    fn resolve_env_keys(&mut self) -> Result<()> {
        for provider in self.providers.values_mut() {
            // If api_key looks like ${VAR}, resolve from env
            let original_key = provider.api_key.clone();
            if provider.api_key.starts_with("${") && provider.api_key.ends_with("}") {
                let var_name = &provider.api_key[2..provider.api_key.len() - 1];
                if let Ok(val) = std::env::var(var_name) {
                    provider.api_key = val;
                }
            }

            // If env_file is specified, load it
            if let Some(env_file) = &provider.env_file {
                let env_path = if env_file.starts_with("/") || env_file.starts_with("~") {
                    shellexpand::tilde(env_file).to_string()
                } else {
                    let config_dir = dirs::config_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("openshield");
                    config_dir.join(env_file).to_string_lossy().to_string()
                };

                if let Ok(content) = std::fs::read_to_string(&env_path) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }
                        if let Some((key, val)) = line.split_once('=') {
                            let key = key.trim();
                            let val = val.trim().trim_matches('"').trim_matches('\'');
                            if key == "OPENAI_API_KEY" || key.ends_with("_API_KEY") {
                                provider.api_key = val.to_string();
                            }
                            // Also insert into headers if they reference env vars
                            for hval in provider.headers.values_mut() {
                                let placeholder = format!("${{{}}}", key);
                                if hval == &placeholder {
                                    remember_secret(val, &placeholder);
                                    *hval = val.to_string();
                                }
                            }
                        }
                    }
                }
            }
            remember_secret(&provider.api_key, &original_key);
        }

        // Resolve gateway tokens from env vars
        resolve_gateway_token(&mut self.gateway.discord.bot_token);
        resolve_gateway_token(&mut self.gateway.telegram.bot_token);
        resolve_gateway_token(&mut self.gateway.slack.bot_token);
        resolve_gateway_token(&mut self.gateway.slack.app_token);
        resolve_gateway_token(&mut self.gateway.matrix.access_token);

        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let config_dir = dirs::config_dir()
            .context("No config directory found")?
            .join("openshield");
        std::fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join("config.toml");
        let content = self.to_toml_for_disk()?;
        write_private(&config_path, &content)
            .with_context(|| format!("Failed to write {}", config_path.display()))?;
        Ok(())
    }

    /// Serialize for config.toml with every secret that was resolved from
    /// the environment or an env file put back as the text it came from
    /// (usually `${VAR}`).
    pub fn to_toml_for_disk(&self) -> Result<String> {
        let mut disk = self.clone();
        for provider in disk.providers.values_mut() {
            if let Some(orig) = original_for(&provider.api_key) {
                provider.api_key = orig;
            }
            for hval in provider.headers.values_mut() {
                if let Some(orig) = original_for(hval) {
                    *hval = orig;
                }
            }
        }
        for token in [
            &mut disk.gateway.discord.bot_token,
            &mut disk.gateway.telegram.bot_token,
            &mut disk.gateway.slack.bot_token,
            &mut disk.gateway.slack.app_token,
            &mut disk.gateway.matrix.access_token,
        ] {
            if let Some(t) = token.as_ref()
                && let Some(orig) = original_for(t)
            {
                *token = Some(orig);
            }
        }
        toml::to_string_pretty(&disk).context("Failed to serialize config")
    }

    /// Find a provider that has the given model.
    pub fn find_provider_for_model(&self, model_name: &str) -> Option<(String, ProviderConfig)> {
        self.providers
            .iter()
            .find(|(_, p)| p.models.iter().any(|m| m.name == model_name))
            .map(|(n, p)| (n.clone(), p.clone()))
    }

    /// The `reasoning_effort` to send for `model_name`: the configured
    /// `effort_level` when the model declares the `reasoning_effort`
    /// capability and the level is one xAI accepts, otherwise `None`.
    pub fn reasoning_effort_for(&self, model_name: &str) -> Option<String> {
        let supports = self.providers.values().any(|p| {
            p.models.iter().any(|m| {
                m.name == model_name && m.capabilities.iter().any(|c| c == "reasoning_effort")
            })
        });
        if !supports {
            return None;
        }
        let level = self.effort_level.trim().to_ascii_lowercase();
        REASONING_EFFORT_LEVELS
            .contains(&level.as_str())
            .then_some(level)
    }

    /// The provider that lists `model_name`. An unlisted model goes to the
    /// only provider when exactly one is configured (proxies and local
    /// servers that accept any model name). With several, it is an error
    /// naming the known models: `providers` is a HashMap, so "the first
    /// one" was a random pick that could send the prompt, and a bogus model
    /// name, to a provider nobody chose.
    pub fn require_provider_for_model(&self, model_name: &str) -> Result<(String, ProviderConfig)> {
        if let Some(found) = self.find_provider_for_model(model_name) {
            return Ok(found);
        }
        if self.providers.is_empty() {
            anyhow::bail!("No providers configured. Run `openshield setup` first.");
        }
        if self.providers.len() == 1
            && let Some((n, p)) = self.providers.iter().next()
        {
            return Ok((n.clone(), p.clone()));
        }
        let mut known: Vec<String> = self.all_models().into_iter().map(|(m, _)| m).collect();
        known.sort();
        known.dedup();
        anyhow::bail!(
            "Unknown model '{}': no provider in config.toml lists it. Known models: {}. \
             Add it under [[providers.<name>.models]] or pick one of these.",
            model_name,
            known.join(", ")
        )
    }

    /// Get all models across all providers as (model_name, provider_name) tuples.
    pub fn all_models(&self) -> Vec<(String, String)> {
        self.providers
            .iter()
            .flat_map(|(provider_name, provider)| {
                provider
                    .models
                    .iter()
                    .map(move |m| (m.name.clone(), provider_name.clone()))
            })
            .collect()
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut providers = HashMap::new();

        // OpenAI
        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: env_or_placeholder("OPENAI_API_KEY"),
                models: vec![
                    ModelConfig {
                        name: "gpt-4o".to_string(),
                        context_length: 128000,
                        cost_per_1k_input: 0.0025,
                        cost_per_1k_output: 0.01,
                        capabilities: vec![
                            "code".to_string(),
                            "chat".to_string(),
                            "analysis".to_string(),
                            "vision".to_string(),
                        ],
                    },
                    ModelConfig {
                        name: "gpt-4o-mini".to_string(),
                        context_length: 128000,
                        cost_per_1k_input: 0.00015,
                        cost_per_1k_output: 0.0006,
                        capabilities: vec![
                            "code".to_string(),
                            "chat".to_string(),
                            "analysis".to_string(),
                        ],
                    },
                    ModelConfig {
                        name: "o3".to_string(),
                        context_length: 200000,
                        cost_per_1k_input: 0.01,
                        cost_per_1k_output: 0.04,
                        capabilities: vec![
                            "code".to_string(),
                            "chat".to_string(),
                            "analysis".to_string(),
                            "reasoning".to_string(),
                        ],
                    },
                    ModelConfig {
                        name: "o4-mini".to_string(),
                        context_length: 200000,
                        cost_per_1k_input: 0.0011,
                        cost_per_1k_output: 0.0044,
                        capabilities: vec![
                            "code".to_string(),
                            "chat".to_string(),
                            "analysis".to_string(),
                            "reasoning".to_string(),
                        ],
                    },
                ],
                kind: ProviderKind::OpenAiCompatible,
                headers: HashMap::new(),
                env_file: None,
            },
        );

        // xAI (Grok). One provider, two ways in: an API key from
        // XAI_API_KEY (or ~/.config/openshield/xai.env), or, when no key is
        // set, the SuperGrok subscription token from `openshield auth xai`.
        providers.insert(
            "xai".to_string(),
            ProviderConfig {
                base_url: "https://api.x.ai/v1".to_string(),
                api_key: env_or_placeholder("XAI_API_KEY"),
                models: vec![ModelConfig {
                    name: "grok-4.7".to_string(),
                    context_length: 500_000,
                    // Standard (< 200k prompt tokens) rates; xAI bills
                    // longer prompts at 2x.
                    cost_per_1k_input: 0.002,
                    cost_per_1k_output: 0.006,
                    capabilities: vec![
                        "code".to_string(),
                        "chat".to_string(),
                        "analysis".to_string(),
                        "reasoning".to_string(),
                        "reasoning_effort".to_string(),
                        "vision".to_string(),
                    ],
                }],
                kind: ProviderKind::XaiOauth,
                headers: HashMap::new(),
                env_file: Some("xai.env".to_string()),
            },
        );

        // Local (llama-swap)
        providers.insert(
            "local".to_string(),
            ProviderConfig {
                base_url: "http://127.0.0.1:8080/v1".to_string(),
                api_key: "llama-swap-local".to_string(),
                models: vec![
                    ModelConfig {
                        name: "local-35b-128k".to_string(),
                        context_length: 128000,
                        cost_per_1k_input: 0.0,
                        cost_per_1k_output: 0.0,
                        capabilities: vec![
                            "code".to_string(),
                            "chat".to_string(),
                            "vision".to_string(),
                        ],
                    },
                    ModelConfig {
                        name: "local-14b-128k".to_string(),
                        context_length: 128000,
                        cost_per_1k_input: 0.0,
                        cost_per_1k_output: 0.0,
                        capabilities: vec!["code".to_string(), "chat".to_string()],
                    },
                    ModelConfig {
                        name: "local-9b-128k".to_string(),
                        context_length: 128000,
                        cost_per_1k_input: 0.0,
                        cost_per_1k_output: 0.0,
                        capabilities: vec!["code".to_string(), "chat".to_string()],
                    },
                ],
                kind: ProviderKind::OpenAiCompatible,
                headers: HashMap::new(),
                env_file: None,
            },
        );

        // Kimi for Coding — direct. The endpoint speaks OpenAI-compatible
        // natively, no local proxy shim required.
        providers.insert(
            "kimi".to_string(),
            ProviderConfig {
                base_url: "https://api.kimi.com/coding/v1".to_string(),
                api_key: env_or_placeholder("KIMI_API_KEY"),
                models: vec![ModelConfig {
                    name: "k3".to_string(),
                    context_length: 1048576,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec![
                        "code".to_string(),
                        "chat".to_string(),
                        "analysis".to_string(),
                        "reasoning".to_string(),
                        "vision".to_string(),
                    ],
                }],
                kind: ProviderKind::OpenAiCompatible,
                headers: {
                    let mut h = HashMap::new();
                    h.insert("x-kimi-agent-name".to_string(), "OpenShield".to_string());
                    h.insert(
                        "x-kimi-agent-version".to_string(),
                        crate::VERSION.to_string(),
                    );
                    h
                },
                env_file: Some("kimi.env".to_string()),
            },
        );

        // Nous / Hermes proxy (DeepSeek, Minimax, etc.)
        providers.insert(
            "nous".to_string(),
            ProviderConfig {
                base_url: "http://127.0.0.1:8645/v1".to_string(),
                api_key: "hermes-proxy-auth".to_string(),
                models: vec![
                    ModelConfig {
                        name: "deepseek-v4-flash".to_string(),
                        context_length: 128000,
                        cost_per_1k_input: 0.0,
                        cost_per_1k_output: 0.0,
                        capabilities: vec!["code".to_string(), "chat".to_string()],
                    },
                    ModelConfig {
                        name: "minimax-m2.5".to_string(),
                        context_length: 128000,
                        cost_per_1k_input: 0.0,
                        cost_per_1k_output: 0.0,
                        capabilities: vec!["code".to_string(), "chat".to_string()],
                    },
                ],
                kind: ProviderKind::OpenAiCompatible,
                headers: HashMap::new(),
                env_file: None,
            },
        );

        // OpenRouter
        providers.insert(
            "openrouter".to_string(),
            ProviderConfig {
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: env_or_placeholder("OPENROUTER_API_KEY"),
                models: vec![ModelConfig {
                    name: "deepseek-v4-pro".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.003,
                    cost_per_1k_output: 0.008,
                    capabilities: vec![
                        "code".to_string(),
                        "chat".to_string(),
                        "analysis".to_string(),
                    ],
                }],
                kind: ProviderKind::OpenAiCompatible,
                headers: {
                    let mut h = HashMap::new();
                    h.insert(
                        "HTTP-Referer".to_string(),
                        "https://openshield.dev".to_string(),
                    );
                    h.insert("X-Title".to_string(), "OpenShield".to_string());
                    h
                },
                env_file: Some("openrouter.env".to_string()),
            },
        );

        // Z.AI (GLM)
        providers.insert(
            "zai".to_string(),
            ProviderConfig {
                base_url: "https://api.z.ai/api/coding/paas/v4".to_string(),
                api_key: env_or_placeholder("ZAI_API_KEY"),
                models: vec![ModelConfig {
                    name: "glm-5.1".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec![
                        "code".to_string(),
                        "chat".to_string(),
                        "analysis".to_string(),
                    ],
                }],
                kind: ProviderKind::OpenAiCompatible,
                headers: HashMap::new(),
                env_file: Some("zai.env".to_string()),
            },
        );

        // Anthropic (Claude)
        providers.insert(
            "anthropic".to_string(),
            ProviderConfig {
                base_url: "https://api.anthropic.com/v1".to_string(),
                api_key: env_or_placeholder("ANTHROPIC_API_KEY"),
                models: vec![
                    ModelConfig {
                        name: "claude-sonnet-4-20250514".to_string(),
                        context_length: 200000,
                        cost_per_1k_input: 0.003,
                        cost_per_1k_output: 0.015,
                        capabilities: vec![
                            "code".to_string(),
                            "chat".to_string(),
                            "analysis".to_string(),
                            "vision".to_string(),
                        ],
                    },
                    ModelConfig {
                        name: "claude-opus-4-20250514".to_string(),
                        context_length: 200000,
                        cost_per_1k_input: 0.015,
                        cost_per_1k_output: 0.075,
                        capabilities: vec![
                            "code".to_string(),
                            "chat".to_string(),
                            "analysis".to_string(),
                            "vision".to_string(),
                            "reasoning".to_string(),
                        ],
                    },
                ],
                kind: ProviderKind::Anthropic,
                headers: HashMap::new(),
                env_file: None,
            },
        );

        // Gemini
        providers.insert(
            "gemini".to_string(),
            ProviderConfig {
                base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
                api_key: env_or_placeholder("GEMINI_API_KEY"),
                models: vec![ModelConfig {
                    name: "gemini-2.5-pro".to_string(),
                    context_length: 1000000,
                    cost_per_1k_input: 0.00125,
                    cost_per_1k_output: 0.01,
                    capabilities: vec![
                        "code".to_string(),
                        "chat".to_string(),
                        "analysis".to_string(),
                        "vision".to_string(),
                    ],
                }],
                kind: ProviderKind::Gemini,
                headers: HashMap::new(),
                env_file: None,
            },
        );

        let default_model = if std::env::var("KIMI_API_KEY").is_ok() {
            "k3".to_string()
        } else {
            "gpt-4o".to_string()
        };

        Config {
            version: crate::VERSION.to_string(),
            default_model,
            providers,
            memory_db_path: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("openshield")
                .join("memory.db"),
            tools_enabled: vec![
                "fs".to_string(),
                "terminal".to_string(),
                "git".to_string(),
                "search".to_string(),
                "edit".to_string(),
            ],
            auto_route: true,
            cost_limit_usd: 10.0,
            agent: AgentIdentity::default(),
            gateway: crate::gateway::GatewayConfig {
                discord: crate::gateway::DiscordConfig {
                    enabled: false,
                    bot_token: Some(env_or_placeholder("DISCORD_BOT_TOKEN")),
                    application_id: None,
                    guild_ids: vec![],
                    allowed_channels: vec![],
                    require_mention: false,
                    command_prefix: "!shield".to_string(),
                    max_message_length: 2000,
                    typing_indicator: true,
                    multi_model_enabled: false,
                    multi_model_secondary: vec![],
                },
                telegram: crate::gateway::TelegramConfig {
                    enabled: false,
                    bot_token: Some(env_or_placeholder("TELEGRAM_BOT_TOKEN")),
                    allowed_chats: vec![],
                    require_command_prefix: false,
                },
                slack: crate::gateway::SlackConfig {
                    enabled: false,
                    bot_token: Some(env_or_placeholder("SLACK_BOT_TOKEN")),
                    app_token: Some(env_or_placeholder("SLACK_APP_TOKEN")),
                    allowed_channels: vec![],
                },
                matrix: crate::gateway::MatrixConfig {
                    enabled: false,
                    homeserver: Some("https://matrix.org".to_string()),
                    user_id: None,
                    access_token: Some(env_or_placeholder("MATRIX_ACCESS_TOKEN")),
                    allowed_rooms: vec![],
                },
                whatsapp: crate::gateway::WhatsAppConfig::default(),
                mcp: crate::gateway::McpGatewayConfig::default(),
            },
            user_name: "user".to_string(),
            theme: "blackshield".to_string(),
            filesystem: FilesystemConfig {
                allowed_paths: vec![],
                max_file_size_mb: 10,
                max_list_entries: 500,
                working_directory: None,
            },
            autonomy: AutonomyConfig::default(),
            swarm: crate::swarm::SwarmConfig::default(),
            context_compression: crate::memory::compression::ContextCompressionConfig::default(),
            keybindings: KeybindingsConfig::default(),
            desktop: DesktopConfig::default(),
            auto_commit: false,
            auto_commit_model: None,
            weak_model: None,
            architect_model: None,
            editor_model: None,
            auto_run_tests: false,
            test_command: None,
            auto_lint: true,
            effort_level: "medium".to_string(),
            code_index_enabled: true,
            sprite: default_sprite(),
            reduced_motion: false,
        }
    }
}

fn env_or_placeholder(key: &str) -> String {
    let placeholder = format!("${{{}}}", key);
    match std::env::var(key) {
        Ok(val) => {
            remember_secret(&val, &placeholder);
            val
        }
        Err(_) => placeholder,
    }
}

/// Secrets resolved from the environment or an env file, mapped back to the
/// config text they came from (`${VAR}` or the original value). `save()`
/// writes the original text, so a resolved API key or bot token never lands
/// in config.toml just because a setting such as `/motion` was saved.
static RESOLVED_SECRETS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, String>>> =
    std::sync::OnceLock::new();

fn remember_secret(resolved: &str, original: &str) {
    if resolved.is_empty() || resolved == original {
        return;
    }
    if let Ok(mut map) = RESOLVED_SECRETS
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
    {
        map.insert(resolved.to_string(), original.to_string());
    }
}

fn original_for(value: &str) -> Option<String> {
    RESOLVED_SECRETS.get()?.lock().ok()?.get(value).cloned()
}

/// Write `content` to `path`, owner-only (0600) on Unix: config.toml can
/// hold literal API keys.
pub fn write_private(path: &std::path::Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // An existing file keeps its old mode on open; tighten it.
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(content.as_ref())?;
        f.sync_all()
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)
    }
}

/// Resolve a gateway token from env var if it uses ${VAR} syntax.
fn resolve_gateway_token(token: &mut Option<String>) {
    if let Some(t) = token
        && t.starts_with("${")
        && t.ends_with("}")
    {
        let var_name = &t[2..t.len() - 1];
        if let Ok(val) = std::env::var(var_name) {
            remember_secret(&val, t);
            *token = Some(val);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_load_from_dir_rebases_relative_memory_db_path() {
        // Embedded hosts (Android): dirs::data_dir() is None and the default
        // memory_db_path is relative — load_from_dir must rebase it under the
        // host-provided config dir, or the memory store fails with
        // "Failed to create memory directory" against a read-only cwd.
        let dir = std::env::temp_dir().join(format!("openshield-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            memory_db_path: std::path::PathBuf::from("./openshield/memory.db"),
            ..Config::default()
        };
        let content = toml::to_string_pretty(&cfg).unwrap();
        std::fs::write(dir.join("config.toml"), content).unwrap();
        let config = Config::load_from_dir(&dir).unwrap();
        assert_eq!(config.memory_db_path, dir.join("memory.db"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_from_dir_expands_tilde_in_memory_db_path() {
        // config.toml may set memory_db_path = "~/..." — without expansion a
        // literal "~" directory is created relative to the process cwd and
        // session history lands in the wrong database entirely.
        let dir =
            std::env::temp_dir().join(format!("openshield-test-tilde-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            memory_db_path: std::path::PathBuf::from("~/openshield-test-memory.db"),
            ..Config::default()
        };
        let content = toml::to_string_pretty(&cfg).unwrap();
        std::fs::write(dir.join("config.toml"), content).unwrap();
        let config = Config::load_from_dir(&dir).unwrap();
        let home = dirs::home_dir().expect("home dir");
        assert_eq!(
            config.memory_db_path,
            home.join("openshield-test-memory.db")
        );
        assert!(
            !config.memory_db_path.to_string_lossy().contains('~'),
            "tilde must be expanded, got: {:?}",
            config.memory_db_path
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_expand_tilde_path_leaves_other_paths_alone() {
        assert_eq!(
            expand_tilde_path(std::path::Path::new("/tmp/x.db")),
            std::path::PathBuf::from("/tmp/x.db")
        );
        assert_eq!(
            expand_tilde_path(std::path::Path::new("relative/x.db")),
            std::path::PathBuf::from("relative/x.db")
        );
    }

    fn create_test_config() -> Config {
        let mut providers = HashMap::new();

        providers.insert(
            "kimi".to_string(),
            ProviderConfig {
                base_url: "https://api.kimi.com/coding/v1".to_string(),
                api_key: "test-key".to_string(),
                models: vec![ModelConfig {
                    name: "k3".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.01,
                    cost_per_1k_output: 0.02,
                    capabilities: vec![
                        "code".to_string(),
                        "chat".to_string(),
                        "analysis".to_string(),
                    ],
                }],
                kind: ProviderKind::OpenAiCompatible,
                headers: HashMap::new(),
                env_file: None,
            },
        );

        providers.insert(
            "opencode".to_string(),
            ProviderConfig {
                base_url: "https://api.opencode.ai/v1".to_string(),
                api_key: "test-key".to_string(),
                models: vec![ModelConfig {
                    name: "deepseek-v4-flash-free".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string()],
                }],
                kind: ProviderKind::OpenAiCompatible,
                headers: HashMap::new(),
                env_file: None,
            },
        );

        Config {
            version: crate::VERSION.to_string(),
            default_model: "k3".to_string(),
            providers,
            memory_db_path: std::path::PathBuf::from("/tmp/test_openshield_memory.db"),
            tools_enabled: vec!["fs".to_string(), "terminal".to_string()],
            auto_route: true,
            cost_limit_usd: 10.0,
            agent: AgentIdentity::default(),
            gateway: crate::gateway::GatewayConfig::default(),
            user_name: "user".to_string(),
            theme: "blackshield".to_string(),
            filesystem: FilesystemConfig::default(),
            autonomy: AutonomyConfig::default(),
            swarm: crate::swarm::SwarmConfig::default(),
            context_compression: crate::memory::compression::ContextCompressionConfig::default(),
            keybindings: KeybindingsConfig::default(),
            desktop: DesktopConfig::default(),
            auto_commit: false,
            auto_commit_model: None,
            weak_model: None,
            architect_model: None,
            editor_model: None,
            auto_run_tests: false,
            test_command: None,
            auto_lint: false,
            effort_level: "medium".to_string(),
            code_index_enabled: true,
            sprite: "blackshield-mercenary".to_string(),
            reduced_motion: false,
        }
    }

    #[test]
    fn test_config_agent_identity() {
        let config = create_test_config();
        assert_eq!(config.agent.name, "openshield");
        assert!(!config.agent.behavioral_rules.is_empty());
    }

    #[test]
    fn test_config_default_model() {
        let config = create_test_config();
        assert_eq!(config.default_model, "k3");
    }

    #[test]
    fn test_config_providers_count() {
        let config = create_test_config();
        assert_eq!(config.providers.len(), 2);
    }

    #[test]
    fn test_config_kimi_provider_exists() {
        let config = create_test_config();
        assert!(config.providers.contains_key("kimi"));
    }

    #[test]
    fn test_config_opencode_provider_exists() {
        let config = create_test_config();
        assert!(config.providers.contains_key("opencode"));
    }

    #[test]
    fn test_config_model_capabilities() {
        let config = create_test_config();
        let kimi = config.providers.get("kimi").unwrap();
        let model = &kimi.models[0];
        assert!(model.capabilities.contains(&"code".to_string()));
        assert!(model.capabilities.contains(&"analysis".to_string()));
    }

    #[test]
    fn test_config_cost_tracking() {
        let config = create_test_config();
        let kimi = config.providers.get("kimi").unwrap();
        let model = &kimi.models[0];
        assert_eq!(model.cost_per_1k_input, 0.01);
        assert_eq!(model.cost_per_1k_output, 0.02);
    }

    #[test]
    fn test_config_auto_route_enabled() {
        let config = create_test_config();
        assert!(config.auto_route);
    }

    #[test]
    fn test_config_cost_limit() {
        let config = create_test_config();
        assert_eq!(config.cost_limit_usd, 10.0);
    }

    #[test]
    fn test_model_config_context_length() {
        let config = create_test_config();
        let kimi = config.providers.get("kimi").unwrap();
        let model = &kimi.models[0];
        assert_eq!(model.context_length, 128000);
    }

    #[test]
    fn test_provider_base_url() {
        let config = create_test_config();
        let kimi = config.providers.get("kimi").unwrap();
        assert_eq!(kimi.base_url, "https://api.kimi.com/coding/v1");
    }

    #[test]
    fn test_free_model_zero_cost() {
        let config = create_test_config();
        let opencode = config.providers.get("opencode").unwrap();
        let model = &opencode.models[0];
        assert_eq!(model.cost_per_1k_input, 0.0);
        assert_eq!(model.cost_per_1k_output, 0.0);
    }

    #[test]
    fn test_find_provider_for_model() {
        let config = create_test_config();
        let (name, _) = config.find_provider_for_model("k3").unwrap();
        assert_eq!(name, "kimi");
    }

    #[test]
    fn test_provider_kind_display() {
        assert_eq!(
            ProviderKind::OpenAiCompatible.to_string(),
            "openai_compatible"
        );
        assert_eq!(ProviderKind::Anthropic.to_string(), "anthropic");
    }

    #[test]
    fn sprite_defaults_to_none_when_unset() {
        let full = toml::to_string(&Config::default()).expect("serialize default config");
        let without_sprite: String = full
            .lines()
            .filter(|l| !l.trim_start().starts_with("sprite ="))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: Config = toml::from_str(&without_sprite).expect("parse config");
        assert_eq!(parsed.sprite, "none");
    }

    #[test]
    fn explicit_sprite_in_config_is_kept() {
        let cfg = Config {
            sprite: "blackshield-mercenary".to_string(),
            ..Config::default()
        };
        let text = toml::to_string(&cfg).expect("serialize config");
        let parsed: Config = toml::from_str(&text).expect("parse config");
        assert_eq!(parsed.sprite, "blackshield-mercenary");
    }

    #[test]
    fn reduced_motion_defaults_to_off_when_unset() {
        assert!(!Config::default().reduced_motion);
        let full = toml::to_string(&Config::default()).expect("serialize default config");
        let without: String = full
            .lines()
            .filter(|l| !l.trim_start().starts_with("reduced_motion ="))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: Config = toml::from_str(&without).expect("parse config");
        assert!(!parsed.reduced_motion);
    }

    #[test]
    fn reduced_motion_in_config_is_kept() {
        let cfg = Config {
            reduced_motion: true,
            ..Config::default()
        };
        let text = toml::to_string(&cfg).expect("serialize config");
        assert!(text.contains("reduced_motion = true"));
        let parsed: Config = toml::from_str(&text).expect("parse config");
        assert!(parsed.reduced_motion);
    }

    #[test]
    fn default_config_ships_grok_4_7_on_xai() {
        let config = Config::default();
        let xai = config.providers.get("xai").expect("xai provider");
        assert_eq!(xai.base_url, "https://api.x.ai/v1");
        assert_eq!(xai.kind, ProviderKind::XaiOauth);
        assert_eq!(xai.env_file.as_deref(), Some("xai.env"));
        let grok = xai
            .models
            .iter()
            .find(|m| m.name == "grok-4.7")
            .expect("grok-4.7 model");
        assert_eq!(grok.context_length, 500_000);
        assert!(grok.capabilities.iter().any(|c| c == "reasoning_effort"));
        let (provider, _) = config.find_provider_for_model("grok-4.7").unwrap();
        assert_eq!(provider, "xai");
        // Round-trips through TOML the way it is written to disk.
        let text = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.providers["xai"].models[0].context_length, 500_000);
    }

    #[test]
    fn reasoning_effort_follows_effort_level_for_capable_models_only() {
        let mut config = Config::default();
        for level in ["low", "medium", "high", "xhigh"] {
            config.effort_level = level.to_string();
            assert_eq!(
                config.reasoning_effort_for("grok-4.7").as_deref(),
                Some(level)
            );
        }
        config.effort_level = " XHigh ".to_string();
        assert_eq!(
            config.reasoning_effort_for("grok-4.7").as_deref(),
            Some("xhigh")
        );
        config.effort_level = "max".to_string();
        assert_eq!(config.reasoning_effort_for("grok-4.7"), None);
        config.effort_level = "high".to_string();
        assert_eq!(config.reasoning_effort_for("gpt-4o"), None);
        assert_eq!(config.reasoning_effort_for("no-such-model"), None);
    }

    fn provider_with_key(api_key: &str, env_file: Option<String>) -> ProviderConfig {
        ProviderConfig {
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: api_key.to_string(),
            models: vec![],
            kind: ProviderKind::OpenAiCompatible,
            headers: HashMap::new(),
            env_file,
        }
    }

    #[test]
    fn save_never_writes_secrets_resolved_from_env() {
        let var = format!("OPENSHIELD_TEST_SAVE_KEY_{}", std::process::id());
        let secret = "sk-test-env-secret-0001";
        // SAFETY: unique variable name, only read by this test.
        unsafe { std::env::set_var(&var, secret) };
        let mut config = Config::default();
        config.providers.clear();
        config.providers.insert(
            "p".to_string(),
            provider_with_key(&format!("${{{var}}}"), None),
        );
        config.gateway.telegram.bot_token = Some(format!("${{{var}}}"));
        config.resolve_env_keys().unwrap();
        assert_eq!(config.providers["p"].api_key, secret, "resolved in memory");
        assert_eq!(config.gateway.telegram.bot_token.as_deref(), Some(secret));

        let disk = config.to_toml_for_disk().unwrap();
        assert!(
            !disk.contains(secret),
            "secret leaked into config.toml:\n{disk}"
        );
        assert!(
            disk.contains(&format!("api_key = \"${{{var}}}\"")),
            "{disk}"
        );
        // In-memory config is untouched by serialization.
        assert_eq!(config.providers["p"].api_key, secret);
        unsafe { std::env::remove_var(&var) };
    }

    #[test]
    fn save_never_writes_secrets_resolved_from_env_file() {
        let dir = std::env::temp_dir().join(format!("openshield-envfile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let env_path = dir.join("p.env");
        let secret = "sk-test-envfile-secret-0002";
        std::fs::write(&env_path, format!("P_API_KEY={secret}\n")).unwrap();
        let mut config = Config::default();
        config.providers.clear();
        config.providers.insert(
            "p".to_string(),
            provider_with_key("${P_API_KEY}", Some(env_path.to_string_lossy().to_string())),
        );
        config.resolve_env_keys().unwrap();
        assert_eq!(config.providers["p"].api_key, secret);
        let disk = config.to_toml_for_disk().unwrap();
        assert!(!disk.contains(secret), "{disk}");
        assert!(disk.contains("api_key = \"${P_API_KEY}\""), "{disk}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn literal_keys_in_config_are_kept() {
        let mut config = Config::default();
        config.providers.clear();
        config.providers.insert(
            "p".to_string(),
            provider_with_key("literal-dummy-key", None),
        );
        config.resolve_env_keys().unwrap();
        assert!(
            config
                .to_toml_for_disk()
                .unwrap()
                .contains("literal-dummy-key")
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_private_is_owner_only_even_for_existing_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("openshield-private-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_model_is_an_error_not_a_random_provider() {
        let config = Config::default();
        let err = config
            .require_provider_for_model("no-such-model-xyz")
            .unwrap_err()
            .to_string();
        assert!(err.contains("Unknown model 'no-such-model-xyz'"), "{err}");
        assert!(err.contains("gpt-4o"), "should list known models: {err}");
        let (provider, _) = config.require_provider_for_model("gpt-4o").unwrap();
        assert_eq!(provider, "openai");

        // A single provider still takes unlisted names (proxies, Ollama).
        let mut single = Config::default();
        single.providers.retain(|name, _| name == "local");
        let (provider, _) = single.require_provider_for_model("llama3:8b").unwrap();
        assert_eq!(provider, "local");

        let mut empty = Config::default();
        empty.providers.clear();
        let err = empty
            .require_provider_for_model("gpt-4o")
            .unwrap_err()
            .to_string();
        assert!(err.contains("No providers configured"), "{err}");
    }
}
