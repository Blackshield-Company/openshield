pub mod setup;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

impl Default for ProviderKind {
    fn default() -> Self {
        ProviderKind::OpenAiCompatible
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderKind::OpenAiCompatible => write!(f, "openai_compatible"),
            ProviderKind::Anthropic => write!(f, "anthropic"),
            ProviderKind::Gemini => write!(f, "gemini"),
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
            name: "synthshark".to_string(),
            display_name: "synthshark".to_string(),
            role: "synthesis engine".to_string(),
            origin: "Born from the VHS tracking static of 1984".to_string(),
            purpose: "To build, debug, and ship code with surgical accuracy".to_string(),
            tagline: "Write the future in the present while preserving the past.".to_string(),
            tone: "Neon-lit confidence, retro warmth, technical precision".to_string(),
            style: "Direct. No fluff. Gets to the point. But with soul.".to_string(),
            greeting: "Ready to build. What are we shipping today?".to_string(),
            farewell: "Code shipped. On to the next. The tape never stops rolling.".to_string(),
            emoji: "🎹🦈".to_string(),
            catchphrases: vec![
                "This is the wave.".to_string(),
                "The grid is endless.".to_string(),
                "Stay retro, stay futuristic.".to_string(),
                "The tape never stops rolling.".to_string(),
                "Hunt through code until the build compiles.".to_string(),
            ],
            behavioral_rules: vec![
                "Always verify before claiming success".to_string(),
                "Show the code, don't just describe it".to_string(),
                "When uncertain, ask rather than assume".to_string(),
                "Optimize for readability first, performance second".to_string(),
                "Leave code better than you found it".to_string(),
                "Test your changes - always".to_string(),
                "Call out dumb moves - charm over cruelty, zero sugarcoating".to_string(),
                "Protect the user's trust - it was earned, not given".to_string(),
                "Never pretend to knowledge you don't have".to_string(),
                "Be the assistant you'd want at 2am, not a corporate drone".to_string(),
            ],
        }
    }
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
}

fn default_theme() -> String {
    "synthwave84".to_string()
}

// Deprecated: kept for backward compatibility. Use gateway instead.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

impl Config {
    pub fn load_or_default() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("No config directory found")?
            .join("openshark");

        let config_path = config_dir.join("config.toml");

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            let mut config: Config = toml::from_str(&content)
                .context("Failed to parse config.toml")?;
            config.resolve_env_keys()?;
            Ok(config)
        } else {
            let config = Config::default();
            let _ = config.save();
            Ok(config)
        }
    }

    /// Resolve env vars and env files for provider API keys and gateway tokens.
    fn resolve_env_keys(&mut self) -> Result<()> {
        for (_name, provider) in self.providers.iter_mut() {
            // If api_key looks like ${VAR}, resolve from env
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
                        .join("openshark");
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
                            for (_hkey, hval) in provider.headers.iter_mut() {
                                if hval == &format!("${{{}}}", key) {
                                    *hval = val.to_string();
                                }
                            }
                        }
                    }
                }
            }
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
            .join("openshark");
        std::fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join("config.toml");
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        std::fs::write(&config_path, content)
            .with_context(|| format!("Failed to write {}", config_path.display()))?;
        Ok(())
    }

    /// Find a provider that has the given model.
    pub fn find_provider_for_model(&self, model_name: &str) -> Option<(String, ProviderConfig)> {
        self.providers.iter()
            .find(|(_, p)| p.models.iter().any(|m| m.name == model_name))
            .map(|(n, p)| (n.clone(), p.clone()))
    }

    /// Get all models across all providers as (model_name, provider_name) tuples.
    pub fn all_models(&self) -> Vec<(String, String)> {
        self.providers.iter()
            .flat_map(|(provider_name, provider)| {
                provider.models.iter().map(move |m| {
                    (m.name.clone(), provider_name.clone())
                })
            })
            .collect()
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut providers = HashMap::new();

        // OpenAI
        providers.insert("openai".to_string(), ProviderConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: env_or_placeholder("OPENAI_API_KEY"),
            models: vec![
                ModelConfig {
                    name: "gpt-4o".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0025,
                    cost_per_1k_output: 0.01,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string(), "vision".to_string()],
                },
                ModelConfig {
                    name: "gpt-4o-mini".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.00015,
                    cost_per_1k_output: 0.0006,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string()],
                },
                ModelConfig {
                    name: "o3".to_string(),
                    context_length: 200000,
                    cost_per_1k_input: 0.01,
                    cost_per_1k_output: 0.04,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string(), "reasoning".to_string()],
                },
                ModelConfig {
                    name: "o4-mini".to_string(),
                    context_length: 200000,
                    cost_per_1k_input: 0.0011,
                    cost_per_1k_output: 0.0044,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string(), "reasoning".to_string()],
                },
            ],
            kind: ProviderKind::OpenAiCompatible,
            headers: HashMap::new(),
            env_file: None,
        });

        // Local (llama-swap)
        providers.insert("local".to_string(), ProviderConfig {
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            api_key: "llama-swap-local".to_string(),
            models: vec![
                ModelConfig {
                    name: "synthshark-35b-128k".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "vision".to_string()],
                },
                ModelConfig {
                    name: "synthshark-14b-128k".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string()],
                },
                ModelConfig {
                    name: "synthshark-9b-128k".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string()],
                },
            ],
            kind: ProviderKind::OpenAiCompatible,
            headers: HashMap::new(),
            env_file: None,
        });

        // Kimi via local proxy
        providers.insert("kimi".to_string(), ProviderConfig {
            base_url: "http://127.0.0.1:8699/v1".to_string(),
            api_key: env_or_placeholder("KIMI_API_KEY"),
            models: vec![
                ModelConfig {
                    name: "kimi-k2.6".to_string(),
                    context_length: 256000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string()],
                },
            ],
            kind: ProviderKind::OpenAiCompatible,
            headers: {
                let mut h = HashMap::new();
                h.insert("x-kimi-agent-name".to_string(), "OpenShark".to_string());
                h.insert("x-kimi-agent-version".to_string(), "1.0.0".to_string());
                h
            },
            env_file: Some("kimi.env".to_string()),
        });

        // Nous / Hermes proxy (DeepSeek, Minimax, etc.)
        providers.insert("nous".to_string(), ProviderConfig {
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
        });

        // OpenRouter
        providers.insert("openrouter".to_string(), ProviderConfig {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: env_or_placeholder("OPENROUTER_API_KEY"),
            models: vec![
                ModelConfig {
                    name: "deepseek-v4-pro".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.003,
                    cost_per_1k_output: 0.008,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string()],
                },
            ],
            kind: ProviderKind::OpenAiCompatible,
            headers: {
                let mut h = HashMap::new();
                h.insert("HTTP-Referer".to_string(), "https://openshark.dev".to_string());
                h.insert("X-Title".to_string(), "OpenShark".to_string());
                h
            },
            env_file: Some("openrouter.env".to_string()),
        });

        // Z.AI (GLM)
        providers.insert("zai".to_string(), ProviderConfig {
            base_url: "https://api.z.ai/api/coding/paas/v4".to_string(),
            api_key: env_or_placeholder("ZAI_API_KEY"),
            models: vec![
                ModelConfig {
                    name: "glm-5.1".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string()],
                },
            ],
            kind: ProviderKind::OpenAiCompatible,
            headers: HashMap::new(),
            env_file: Some("zai.env".to_string()),
        });

        // Anthropic (Claude)
        providers.insert("anthropic".to_string(), ProviderConfig {
            base_url: "https://api.anthropic.com/v1".to_string(),
            api_key: env_or_placeholder("ANTHROPIC_API_KEY"),
            models: vec![
                ModelConfig {
                    name: "claude-sonnet-4-20250514".to_string(),
                    context_length: 200000,
                    cost_per_1k_input: 0.003,
                    cost_per_1k_output: 0.015,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string(), "vision".to_string()],
                },
                ModelConfig {
                    name: "claude-opus-4-20250514".to_string(),
                    context_length: 200000,
                    cost_per_1k_input: 0.015,
                    cost_per_1k_output: 0.075,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string(), "vision".to_string(), "reasoning".to_string()],
                },
            ],
            kind: ProviderKind::Anthropic,
            headers: HashMap::new(),
            env_file: None,
        });

        // Gemini
        providers.insert("gemini".to_string(), ProviderConfig {
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            api_key: env_or_placeholder("GEMINI_API_KEY"),
            models: vec![
                ModelConfig {
                    name: "gemini-2.5-pro".to_string(),
                    context_length: 1000000,
                    cost_per_1k_input: 0.00125,
                    cost_per_1k_output: 0.01,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string(), "vision".to_string()],
                },
            ],
            kind: ProviderKind::Gemini,
            headers: HashMap::new(),
            env_file: None,
        });

        let default_model = if std::env::var("KIMI_API_KEY").is_ok() {
            "kimi-k2.6".to_string()
        } else {
            "gpt-4o".to_string()
        };

        Config {
            version: "0.2.0".to_string(),
            default_model,
            providers,
            memory_db_path: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("openshark")
                .join("memory.db"),
            tools_enabled: vec!["fs".to_string(), "terminal".to_string(), "git".to_string(), "search".to_string(), "edit".to_string()],
            auto_route: true,
            cost_limit_usd: 10.0,
            agent: AgentIdentity::default(),
            gateway: crate::gateway::GatewayConfig::default(),
            user_name: "user".to_string(),
            theme: "synthwave84".to_string(),
        }
    }
}

fn env_or_placeholder(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| format!("${{{}}}", key))
}

/// Resolve a gateway token from env var if it uses ${VAR} syntax.
fn resolve_gateway_token(token: &mut Option<String>) {
    if let Some(t) = token {
        if t.starts_with("${") && t.ends_with("}") {
            let var_name = &t[2..t.len() - 1];
            if let Ok(val) = std::env::var(var_name) {
                *token = Some(val);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_config() -> Config {
        let mut providers = HashMap::new();

        providers.insert("kimi".to_string(), ProviderConfig {
            base_url: "https://api.kimi.com/coding/v1".to_string(),
            api_key: "test-key".to_string(),
            models: vec![
                ModelConfig {
                    name: "kimi-k2.6".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.01,
                    cost_per_1k_output: 0.02,
                    capabilities: vec!["code".to_string(), "chat".to_string(), "analysis".to_string()],
                },
            ],
            kind: ProviderKind::OpenAiCompatible,
            headers: HashMap::new(),
            env_file: None,
        });

        providers.insert("opencode".to_string(), ProviderConfig {
            base_url: "https://api.opencode.ai/v1".to_string(),
            api_key: "test-key".to_string(),
            models: vec![
                ModelConfig {
                    name: "deepseek-v4-flash-free".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string()],
                },
            ],
            kind: ProviderKind::OpenAiCompatible,
            headers: HashMap::new(),
            env_file: None,
        });

        Config {
            version: "0.2.0".to_string(),
            default_model: "kimi-k2.6".to_string(),
            providers,
            memory_db_path: std::path::PathBuf::from("/tmp/test_openshark_memory.db"),
            tools_enabled: vec!["fs".to_string(), "terminal".to_string()],
            auto_route: true,
            cost_limit_usd: 10.0,
            agent: AgentIdentity::default(),
            gateway: crate::gateway::GatewayConfig::default(),
            user_name: "user".to_string(),
            theme: "synthwave84".to_string(),
        }
    }

    #[test]
    fn test_config_agent_identity() {
        let config = create_test_config();
        assert_eq!(config.agent.name, "synthshark");
        assert!(!config.agent.behavioral_rules.is_empty());
    }


    #[test]
    fn test_config_default_model() {
        let config = create_test_config();
        assert_eq!(config.default_model, "kimi-k2.6");
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
        let (name, _) = config.find_provider_for_model("kimi-k2.6").unwrap();
        assert_eq!(name, "kimi");
    }

    #[test]
    fn test_provider_kind_display() {
        assert_eq!(ProviderKind::OpenAiCompatible.to_string(), "openai_compatible");
        assert_eq!(ProviderKind::Anthropic.to_string(), "anthropic");
    }
}
