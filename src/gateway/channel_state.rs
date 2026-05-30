use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::providers::{Message, Provider};

/// Per-channel conversation state.
#[derive(Clone)]
pub struct ChannelState {
    /// Conversation history (system + user + assistant messages).
    pub history: Vec<Message>,
    /// Current active model name.
    pub model: String,
    /// Current provider for this channel.
    pub provider: Provider,
    /// Custom system prompt (if set).
    pub custom_system_prompt: Option<String>,
    /// Whether typing indicator is enabled.
    pub typing_indicator: bool,
    /// Max history length before truncation.
    pub max_history: usize,
    /// Whether @mention is required.
    pub require_mention: bool,
}

impl ChannelState {
    pub fn new(config: &Config) -> Self {
        let model = config.default_model.clone();
        let (provider_name, provider_config) =
            config.find_provider_for_model(&model).unwrap_or_else(|| {
                config
                    .providers
                    .iter()
                    .next()
                    .map(|(name, cfg)| (name.clone(), cfg.clone()))
                    .unwrap_or_else(|| {
                        (
                            "local".to_string(),
                            crate::config::ProviderConfig {
                                base_url: "http://127.0.0.1:8080/v1".to_string(),
                                api_key: "local".to_string(),
                                models: vec![],
                                kind: crate::config::ProviderKind::OpenAiCompatible,
                                headers: HashMap::new(),
                                env_file: None,
                            },
                        )
                    })
            });

        let provider = Provider::new(
            provider_name.clone(),
            provider_config.base_url.clone(),
            provider_config.api_key.clone(),
            provider_config.kind.clone(),
            provider_config.headers.clone(),
        );

        let soul = crate::agent::soul::load_soul_from_config(config);
        let system_msg = Message {
            role: "system".to_string(),
            content: format!(
                "{}\n\nYou are chatting in Discord. Be concise. Use markdown.\n\
                 You have access to tools:\n{}\n\
                 When you need to use a tool, respond with: TOOL:tool_name args",
                soul.system_prompt(),
                crate::tools::get_tools()
                    .iter()
                    .map(|t| format!("- {}: {}", t.name(), t.description()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        };

        Self {
            history: vec![system_msg],
            model,
            provider,
            custom_system_prompt: None,
            typing_indicator: config.gateway.discord.typing_indicator,
            max_history: 20,
            require_mention: config.gateway.discord.require_mention,
        }
    }

    /// Reset to default state (clear history, restore default system prompt).
    pub fn reset(&mut self, config: &Config) {
        let soul = crate::agent::soul::load_soul_from_config(config);
        let system_msg = Message {
            role: "system".to_string(),
            content: format!(
                "{}\n\nYou are chatting in Discord. Be concise. Use markdown.\n\
                 You have access to tools:\n{}\n\
                 When you need to use a tool, respond with: TOOL:tool_name args",
                soul.system_prompt(),
                crate::tools::get_tools()
                    .iter()
                    .map(|t| format!("- {}: {}", t.name(), t.description()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        };
        self.history = vec![system_msg];
        self.custom_system_prompt = None;
    }

    /// Set a custom system prompt.
    pub fn set_system_prompt(&mut self, prompt: &str) {
        self.custom_system_prompt = Some(prompt.to_string());
        // Replace the first system message
        if let Some(first) = self.history.first_mut() {
            if first.role == "system" {
                first.content = prompt.to_string();
            } else {
                self.history.insert(
                    0,
                    Message {
                        role: "system".to_string(),
                        content: prompt.to_string(),
                    },
                );
            }
        }
    }

    /// Switch the active model.
    pub fn switch_model(&mut self, model_name: &str, config: &Config) -> anyhow::Result<()> {
        for (provider_name, provider_cfg) in &config.providers {
            if let Some(model_cfg) = provider_cfg.models.iter().find(|m| m.name == model_name) {
                self.model = model_cfg.name.clone();
                self.provider = Provider::new(
                    provider_name.clone(),
                    provider_cfg.base_url.clone(),
                    provider_cfg.api_key.clone(),
                    provider_cfg.kind.clone(),
                    provider_cfg.headers.clone(),
                );
                return Ok(());
            }
        }
        anyhow::bail!("Model '{}' not found in config", model_name)
    }

    /// Add a user message and trim history if needed.
    pub fn add_user_message(&mut self, content: String) {
        self.history.push(Message {
            role: "user".to_string(),
            content,
        });
        self.trim_history();
    }

    /// Add an assistant message and trim history if needed.
    pub fn add_assistant_message(&mut self, content: String) {
        self.history.push(Message {
            role: "assistant".to_string(),
            content,
        });
        self.trim_history();
    }

    /// Add a tool result as a user message.
    pub fn add_tool_result(&mut self, tool_name: &str, result: &str) {
        self.history.push(Message {
            role: "user".to_string(),
            content: format!("Tool '{}' result: {}", tool_name, result),
        });
        self.trim_history();
    }

    fn trim_history(&mut self) {
        // Always keep system message at index 0
        if self.history.len() > self.max_history + 1 {
            let system = self.history.remove(0);
            let excess = self.history.len() - self.max_history;
            self.history = self.history.split_off(excess);
            self.history.insert(0, system);
        }
    }

    /// Get all messages for a chat request.
    pub fn get_messages(&self) -> Vec<Message> {
        self.history.clone()
    }
}

/// Thread-safe store of per-channel state.
#[derive(Clone)]
pub struct ChannelStateStore {
    states: Arc<Mutex<HashMap<u64, ChannelState>>>,
    config: Config,
}

impl ChannelStateStore {
    pub fn new(config: Config) -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Get or create channel state.
    pub fn get_or_create(&self, channel_id: u64) -> ChannelState {
        let mut states = self.states.lock().unwrap();
        states
            .entry(channel_id)
            .or_insert_with(|| ChannelState::new(&self.config))
            .clone()
    }

    /// Update channel state.
    pub fn update(&self, channel_id: u64, state: ChannelState) {
        let mut states = self.states.lock().unwrap();
        states.insert(channel_id, state);
    }

    /// Remove channel state (for /new).
    pub fn remove(&self, channel_id: u64) {
        let mut states = self.states.lock().unwrap();
        states.remove(&channel_id);
    }

    /// Check if a channel has state.
    pub fn has(&self, channel_id: u64) -> bool {
        let states = self.states.lock().unwrap();
        states.contains_key(&channel_id)
    }
}
