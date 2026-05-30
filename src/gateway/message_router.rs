use anyhow::Result;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::gateway::discord::DiscordEvent;
use crate::memory::{MemoryStore, Message as MemoryMessage};
use crate::providers::{ChatRequest, Message, Provider};
use crate::tools::get_tools;

/// Routes incoming Discord messages to the OpenShark chat engine
/// and streams responses back to Discord.
pub struct MessageRouter {
    config: Config,
    memory: MemoryStore,
    provider: Provider,
    /// Per-channel conversation history (channel_id -> messages).
    channel_history: HashMap<u64, Vec<Message>>,
    /// Per-channel system prompts (channel_id -> system message).
    channel_system: HashMap<u64, Message>,
}

impl MessageRouter {
    pub fn new(config: Config) -> Result<Self> {
        let (provider_name, provider_config) = config
            .find_provider_for_model(&config.default_model)
            .unwrap_or_else(|| {
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

        let memory = MemoryStore::new(&config.memory_db_path)?;

        Ok(Self {
            config,
            memory,
            provider,
            channel_history: HashMap::new(),
            channel_system: HashMap::new(),
        })
    }

    /// Handle a Discord event and stream the response back.
    pub async fn handle_event(
        &mut self,
        event: DiscordEvent,
    ) {
        match event {
            DiscordEvent::UserMessage {
                channel_id,
                user_id,
                username,
                content,
                reply_tx,
            } => {
                if let Err(e) = self
                    .handle_user_message(channel_id, user_id, username, content, reply_tx)
                    .await
                {
                    error!("Failed to handle user message: {}", e);
                }
            }
            DiscordEvent::SlashCommand { interaction, reply_tx } => {
                if let Err(e) = self.handle_slash_command(interaction, reply_tx).await {
                    error!("Failed to handle slash command: {}", e);
                }
            }
            DiscordEvent::Ready => {
                info!("Discord gateway ready");
            }
            DiscordEvent::Disconnected => {
                warn!("Discord gateway disconnected");
            }
        }
    }

    async fn handle_user_message(
        &mut self,
        channel_id: u64,
        _user_id: u64,
        username: String,
        content: String,
        reply_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Build or retrieve system prompt for this channel
        let system_msg = self
            .channel_system
            .entry(channel_id)
            .or_insert_with(|| {
                let soul = crate::agent::soul::load_soul_from_config(&self.config);
                Message {
                    role: "system".to_string(),
                    content: format!(
                        "{}\n\nYou are chatting in Discord. Be concise. Use markdown.\n\
                         You have access to tools:\n{}\n\
                         When you need to use a tool, respond with: TOOL:tool_name args",
                        soul.system_prompt(),
                        get_tools()
                            .iter()
                            .map(|t| format!("- {}: {}", t.name(), t.description()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                }
            })
            .clone();

        // Build message history for this channel
        let history = self.channel_history.entry(channel_id).or_default();

        // Add user message
        history.push(Message {
            role: "user".to_string(),
            content: format!("{}: {}", username, content),
        });

        // Trim history to avoid context overflow
        const MAX_HISTORY: usize = 20;
        if history.len() > MAX_HISTORY {
            *history = history.split_off(history.len() - MAX_HISTORY);
        }

        // Build full message list
        let mut messages = vec![system_msg.clone()];
        messages.extend(history.clone());

        // Drop the mutable borrow before streaming
        let _ = history;

        // Create chat request
        let req = ChatRequest::new(self.config.default_model.clone(), messages, true);

        // Stream response
        let tool_result = match self.provider.chat_stream(req).await {
            Ok((chunks, _metrics)) => {
                let full_response: String = chunks.join("");
                let tool_result = self.try_execute_tools(&full_response).await;
                (full_response, tool_result)
            }
            Err(e) => {
                let _ = reply_tx.send(format!("Error: {}", e));
                return Ok(());
            }
        };

        let full_response = tool_result.0;

        // Re-borrow history to push results
        let history = self.channel_history.entry(channel_id).or_default();

        if let Some(tool_result) = tool_result.1 {
            history.push(Message {
                role: "assistant".to_string(),
                content: full_response.clone(),
            });
            history.push(Message {
                role: "user".to_string(),
                content: format!("Tool result: {}", tool_result),
            });

            let mut messages = vec![system_msg];
            messages.extend(history.clone());
            let req = ChatRequest::new(self.config.default_model.clone(), messages, true);

            match self.provider.chat_stream(req).await {
                Ok((chunks, _metrics)) => {
                    let follow_up: String = chunks.join("");
                    history.push(Message {
                        role: "assistant".to_string(),
                        content: follow_up.clone(),
                    });
                    let _ = reply_tx.send(follow_up);
                }
                Err(e) => {
                    let _ = reply_tx.send(format!("Error: {}", e));
                }
            }
        } else {
            history.push(Message {
                role: "assistant".to_string(),
                content: full_response.clone(),
            });
            let _ = reply_tx.send(full_response);
        }

        // Persist to memory
        let session_id = format!("discord-{}", channel_id);
        let _ = self.memory.save_message(&crate::memory::Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            role: "user".to_string(),
            content: format!("{}: {}", username, content),
            created_at: chrono::Utc::now(),
            tokens_used: None,
        });

        Ok(())
    }

    async fn handle_slash_command(
        &mut self,
        interaction: serenity::all::Interaction,
        reply_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        if let Some(cmd) = interaction.as_command() {
            let name = cmd.data.name.clone();

            match name.as_str() {
                "chat" => {
                    let content = cmd
                        .data
                        .options
                        .iter()
                        .find(|o| o.name == "message")
                        .and_then(|o| match &o.value {
                            serenity::all::CommandDataOptionValue::String(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .unwrap_or("Hello!");

                    let channel_id = cmd.channel_id.get();
                    let user_id = cmd.user.id.get();
                    let username = cmd.user.name.clone();

                    self.handle_user_message(
                        channel_id,
                        user_id,
                        username,
                        content.to_string(),
                        reply_tx,
                    )
                    .await?;
                }
                "model" => {
                    let model_name = cmd
                        .data
                        .options
                        .iter()
                        .find(|o| o.name == "name")
                        .and_then(|o| match &o.value {
                            serenity::all::CommandDataOptionValue::String(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .unwrap_or("");

                    if model_name.is_empty() {
                        let models: Vec<String> = self
                            .config
                            .providers
                            .iter()
                            .flat_map(|(provider_name, provider)| {
                                provider.models.iter().map(move |m| {
                                    format!("{} ({})", m.name, provider_name)
                                })
                            })
                            .collect();
                        let _ = reply_tx.send(format!(
                            "Available models:\n{}",
                            models.join("\n")
                        ));
                    } else {
                        // Switch model logic would go here
                        let _ = reply_tx.send(format!("Model switched to: {}", model_name));
                    }
                }
                "status" => {
                    let _ = reply_tx.send("🦈 OpenShark is online and ready.".to_string());
                }
                _ => {
                    let _ = reply_tx.send(format!("Unknown command: {}", name));
                }
            }
        }

        Ok(())
    }

    /// Try to execute any TOOL: invocations in the response.
    async fn try_execute_tools(&self, response: &str) -> Option<String> {
        use crate::tools::find_tool;

        if let Some(tool_start) = response.find("TOOL:") {
            let tool_line = &response[tool_start..];
            let tool_line = tool_line.lines().next()?;
            let rest = &tool_line[5..]; // Strip "TOOL:"
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.is_empty() {
                return None;
            }

            let tool_name = parts[0].trim();
            let args = parts.get(1).unwrap_or(&"").trim();

            if let Some(tool) = find_tool(tool_name) {
                match tool.execute(args) {
                    Ok(result) => Some(result),
                    Err(e) => Some(format!("Tool error: {}", e)),
                }
            } else {
                Some(format!("Unknown tool: {}", tool_name))
            }
        } else {
            None
        }
    }
}
