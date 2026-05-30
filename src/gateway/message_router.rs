use anyhow::Result;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::gateway::channel_state::{ChannelState, ChannelStateStore};
use crate::gateway::discord::DiscordEvent;
use crate::memory::{MemoryStore, Message as MemoryMessage};
use crate::providers::{ChatRequest, Message, Provider};
use crate::tools::{find_tool, get_tools};

/// Routes incoming Discord messages and slash commands to the OpenShark engine.
pub struct MessageRouter {
    config: Config,
    memory: MemoryStore,
    channel_states: ChannelStateStore,
}

impl MessageRouter {
    pub fn new(config: Config) -> Result<Self> {
        let memory = MemoryStore::new(&config.memory_db_path)?;
        let channel_states = ChannelStateStore::new(config.clone());

        Ok(Self {
            config,
            memory,
            channel_states,
        })
    }

    /// Handle a Discord event and stream the response back.
    pub async fn handle_event(&mut self, event: DiscordEvent) {
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
        // Get or create channel state
        let mut state = self.channel_states.get_or_create(channel_id);

        // Add user message
        state.add_user_message(format!("{}: {}", username, content));

        // Persist to memory
        let session_id = format!("discord-{}", channel_id);
        let _ = self.memory.save_message(&MemoryMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            role: "user".to_string(),
            content: format!("{}: {}", username, content),
            created_at: chrono::Utc::now(),
            tokens_used: None,
        });

        // Stream response
        let messages = state.get_messages();
        let req = ChatRequest::new(state.model.clone(), messages, true);
        let provider = state.provider.clone();

        let tool_result = match provider.chat_stream(req).await {
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

        // Handle tool execution and follow-up
        if let Some(tool_result) = tool_result.1 {
            state.add_assistant_message(full_response.clone());
            state.add_tool_result("tool", &tool_result);

            let messages = state.get_messages();
            let req = ChatRequest::new(state.model.clone(), messages, true);

            match provider.chat_stream(req).await {
                Ok((chunks, _metrics)) => {
                    let follow_up: String = chunks.join("");
                    state.add_assistant_message(follow_up.clone());
                    let _ = reply_tx.send(follow_up);
                }
                Err(e) => {
                    let _ = reply_tx.send(format!("Error: {}", e));
                }
            }
        } else {
            state.add_assistant_message(full_response.clone());
            let _ = reply_tx.send(full_response);
        }

        // Save updated state
        self.channel_states.update(channel_id, state);

        Ok(())
    }

    async fn handle_slash_command(
        &mut self,
        interaction: serenity::all::Interaction,
        reply_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        if let Some(cmd) = interaction.as_command() {
            let name = cmd.data.name.clone();
            let channel_id = cmd.channel_id.get();

            match name.as_str() {
                // ─── Core Chat ───
                "chat" => {
                    let content = get_string_option(&cmd.data.options, "message")
                        .unwrap_or("Hello!");
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

                "new" => {
                    self.channel_states.remove(channel_id);
                    let _ = reply_tx.send(
                        "🆕 Fresh conversation started. History cleared.".to_string(),
                    );
                }

                "system" => {
                    if let Some(prompt) = get_string_option(&cmd.data.options, "prompt") {
                        let mut state = self.channel_states.get_or_create(channel_id);
                        state.set_system_prompt(prompt);
                        self.channel_states.update(channel_id, state);
                        let _ = reply_tx.send(
                            "📝 System prompt updated for this channel.".to_string(),
                        );
                    } else {
                        let _ = reply_tx.send(
                            "❌ Please provide a prompt. Usage: `/system prompt:your prompt here`"
                                .to_string(),
                        );
                    }
                }

                "reset" => {
                    let mut state = self.channel_states.get_or_create(channel_id);
                    state.reset(&self.config);
                    self.channel_states.update(channel_id, state);
                    let _ = reply_tx.send(
                        "🔄 Reset complete. Default system prompt restored and history cleared."
                            .to_string(),
                    );
                }

                // ─── Model Management ───
                "model" => {
                    let mut state = self.channel_states.get_or_create(channel_id);

                    if let Some(model_name) = get_string_option(&cmd.data.options, "name") {
                        match state.switch_model(model_name, &self.config) {
                            Ok(()) => {
                                self.channel_states.update(channel_id, state);
                                let _ = reply_tx.send(format!(
                                    "🤖 Model switched to: **{}**",
                                    model_name
                                ));
                            }
                            Err(e) => {
                                let _ = reply_tx.send(format!("❌ {}", e));
                            }
                        }
                    } else {
                        // List models
                        let current_model = state.model.clone();
                        let mut models = Vec::new();
                        for (provider_name, provider) in &self.config.providers {
                            for m in &provider.models {
                                let marker = if m.name == current_model { "●" } else { "○" };
                                models.push(format!(
                                    "{} **{}** | ctx={} | {} | {}",
                                    marker,
                                    m.name,
                                    m.context_length,
                                    provider_name,
                                    m.capabilities.join(", ")
                                ));
                            }
                        }

                        let current = format!("Current model: **{}**\n\n", current_model);
                        let _ = reply_tx.send(format!(
                            "{}Available models:\n{}",
                            current,
                            models.join("\n")
                        ));
                    }
                }

                "models" => {
                    let mut lines = vec!["🤖 **Available Models**\n".to_string()];

                    for (provider_name, provider) in &self.config.providers {
                        lines.push(format!("\n**{}** ({})", provider_name, provider.base_url));
                        for model in &provider.models {
                            let cost = model.cost_per_1k_input + model.cost_per_1k_output;
                            let cost_str = if cost > 0.0 {
                                format!("${:.4}/1K", cost)
                            } else {
                                "Free".to_string()
                            };
                            let default_marker =
                                if model.name == self.config.default_model {
                                    " [default]"
                                } else {
                                    ""
                                };
                            lines.push(format!(
                                "  • `{}` | ctx={} | {} | capabilities: {}{}",
                                model.name,
                                model.context_length,
                                cost_str,
                                model.capabilities.join(", "),
                                default_marker
                            ));
                        }
                    }

                    let _ = reply_tx.send(lines.join("\n"));
                }

                // ─── Agent ───
                "agent" => {
                    if let Some(task) = get_string_option(&cmd.data.options, "task") {
                        let _ = reply_tx.send(
                            format!("🦈 Starting agent task: **{}**\nThis may take a moment...", task)
                        );

                        // Run agent task
                        let agent_config = crate::agent::AgentConfig::default();
                        let agent = crate::agent::Agent::new(agent_config, &self.config)?;

                        match agent.run_task(task).await {
                            Ok(result) => {
                                let mut msg = format!(
                                    "\n**Agent Result:** {}\n",
                                    if result.success {
                                        "✅ Success"
                                    } else {
                                        "⚠️ Partial"
                                    }
                                );
                                msg.push_str(&format!("{}\n", result.message));
                                msg.push_str(&format!(
                                    "Total iterations: {}\n",
                                    result.total_iterations
                                ));
                                for (i, step) in result.step_results.iter().enumerate() {
                                    msg.push_str(&format!(
                                        "  {}. `{} {}` → verified={} ({} iter)\n",
                                        i + 1,
                                        step.step.tool_name,
                                        step.step.args,
                                        step.verified,
                                        step.iterations
                                    ));
                                }
                                let _ = reply_tx.send(msg);
                            }
                            Err(e) => {
                                let _ = reply_tx.send(format!("❌ Agent error: {}", e));
                            }
                        }
                    } else {
                        let _ = reply_tx.send(
                            "❌ Please provide a task. Usage: `/agent task:your task here`"
                                .to_string(),
                        );
                    }
                }

                // ─── Tools ───
                "tools" => {
                    let tools = get_tools();
                    let mut lines = vec!["🔧 **Available Tools**\n".to_string()];
                    for tool in tools {
                        lines.push(format!("• `{}`: {}", tool.name(), tool.description()));
                    }
                    lines.push("\nUse `/tool name:<tool> args:<arguments>` to execute directly.".to_string());
                    let _ = reply_tx.send(lines.join("\n"));
                }

                "tool" => {
                    let tool_name = get_string_option(&cmd.data.options, "name").unwrap_or("");
                    let args = get_string_option(&cmd.data.options, "args").unwrap_or("");

                    if tool_name.is_empty() {
                        let _ = reply_tx.send(
                            "❌ Usage: `/tool name:<tool_name> args:<arguments>`".to_string(),
                        );
                        return Ok(());
                    }

                    if let Some(tool) = find_tool(tool_name) {
                        let _ = reply_tx.send(format!(
                            "🔧 Executing `{} {}`...",
                            tool_name, args
                        ));
                        match tool.execute(args) {
                            Ok(result) => {
                                let display = if result.len() > 1800 {
                                    format!("{}\n\n... (truncated)", &result[..1800])
                                } else {
                                    result
                                };
                                let _ = reply_tx.send(format!(
                                    "✅ **Result:**\n```\n{}\n```",
                                    display
                                ));
                            }
                            Err(e) => {
                                let _ = reply_tx.send(format!("❌ Tool error: {}", e));
                            }
                        }
                    } else {
                        let _ = reply_tx.send(format!(
                            "❌ Unknown tool: `{}`. Use `/tools` to list available tools.",
                            tool_name
                        ));
                    }
                }

                // ─── Memory ───
                "memory" => {
                    if let Some(query) = get_string_option(&cmd.data.options, "query") {
                        match self.memory.search_messages(query, 10) {
                            Ok(messages) => {
                                if messages.is_empty() {
                                    let _ = reply_tx.send(
                                        "🔍 No memories found for that query.".to_string(),
                                    );
                                } else {
                                    let mut lines = vec![format!(
                                        "🔍 **Memory Search:** '{}' ({} results)\n",
                                        query,
                                        messages.len()
                                    )];
                                    for msg in messages {
                                        let preview = if msg.content.len() > 200 {
                                            format!("{}...", &msg.content[..200])
                                        } else {
                                            msg.content.clone()
                                        };
                                        lines.push(format!(
                                            "[{}] **{}**: {}",
                                            msg.created_at.format("%Y-%m-%d %H:%M"),
                                            msg.role,
                                            preview
                                        ));
                                    }
                                    let _ = reply_tx.send(lines.join("\n"));
                                }
                            }
                            Err(e) => {
                                let _ = reply_tx.send(format!("❌ Memory search error: {}", e));
                            }
                        }
                    } else {
                        let _ = reply_tx.send(
                            "❌ Usage: `/memory query:your search query`".to_string(),
                        );
                    }
                }

                "remember" => {
                    if let Some(fact) = get_string_option(&cmd.data.options, "fact") {
                        let session_id = format!("discord-{}", channel_id);
                        let msg = MemoryMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            session_id,
                            role: "user".to_string(),
                            content: format!("[REMEMBERED] {}", fact),
                            created_at: chrono::Utc::now(),
                            tokens_used: None,
                        };
                        match self.memory.save_message(&msg) {
                            Ok(()) => {
                                let _ = reply_tx.send(
                                    "💾 Fact saved to long-term memory.".to_string(),
                                );
                            }
                            Err(e) => {
                                let _ = reply_tx.send(format!("❌ Failed to save: {}", e));
                            }
                        }
                    } else {
                        let _ = reply_tx.send(
                            "❌ Usage: `/remember fact:the fact to remember`".to_string(),
                        );
                    }
                }

                // ─── Status / Info ───
                "status" => {
                    let state = self.channel_states.get_or_create(channel_id);
                    let mut lines = vec!["🦈 **OpenShark Status**\n".to_string()];
                    lines.push(format!("Model: `{}`", state.model));
                    lines.push(format!(
                        "History: {} messages (max: {})",
                        state.history.len().saturating_sub(1),
                        state.max_history
                    ));
                    lines.push(format!(
                        "Typing indicator: {}",
                        if state.typing_indicator { "✅" } else { "❌" }
                    ));
                    lines.push(format!(
                        "Require mention: {}",
                        if state.require_mention { "✅" } else { "❌" }
                    ));
                    if state.custom_system_prompt.is_some() {
                        lines.push("Custom system prompt: ✅".to_string());
                    }
                    lines.push(format!("\nVersion: {}", self.config.version));
                    let _ = reply_tx.send(lines.join("\n"));
                }

                "stats" => {
                    match self.memory.get_stats_summary() {
                        Ok(stats) => {
                            let mut lines = vec!["📊 **OpenShark Stats**\n".to_string()];
                            lines.push(format!("Total Sessions: {}", stats.total_sessions));
                            lines.push(format!("Total Messages: {}", stats.total_messages));
                            lines.push(format!("Total Tool Calls: {}", stats.total_tool_calls));
                            lines.push(format!(
                                "Successful Tools: {} ({:.1}%)",
                                stats.successful_tool_calls, stats.tool_success_rate
                            ));
                            lines.push(format!("Total Tokens: {}", stats.total_tokens));
                            lines.push(format!("Unique Models: {}", stats.unique_models));
                            if let Some(latest) = stats.latest_session {
                                lines.push(format!(
                                    "Latest Session: {}",
                                    latest.format("%Y-%m-%d %H:%M")
                                ));
                            }
                            let _ = reply_tx.send(lines.join("\n"));
                        }
                        Err(e) => {
                            let _ = reply_tx.send(format!("❌ Error loading stats: {}", e));
                        }
                    }
                }

                // ─── Settings ───
                "settings" => {
                    let mut state = self.channel_states.get_or_create(channel_id);
                    let key = get_string_option(&cmd.data.options, "key");
                    let value = get_string_option(&cmd.data.options, "value");

                    if let (Some(k), Some(v)) = (key, value) {
                        match k {
                            "typing_indicator" => {
                                state.typing_indicator = v == "true" || v == "on";
                                let _ = reply_tx.send(format!(
                                    "Typing indicator: {}",
                                    if state.typing_indicator { "✅" } else { "❌" }
                                ));
                            }
                            "max_history" => {
                                if let Ok(n) = v.parse::<usize>() {
                                    state.max_history = n.clamp(5, 100);
                                    let _ = reply_tx.send(format!(
                                        "Max history: {} messages",
                                        state.max_history
                                    ));
                                } else {
                                    let _ = reply_tx.send(
                                        "❌ max_history must be a number (5-100)".to_string(),
                                    );
                                }
                            }
                            "require_mention" => {
                                state.require_mention = v == "true" || v == "on";
                                let _ = reply_tx.send(format!(
                                    "Require mention: {}",
                                    if state.require_mention { "✅" } else { "❌" }
                                ));
                            }
                            _ => {
                                let _ = reply_tx.send(
                                    "❌ Unknown setting. Available: typing_indicator, max_history, require_mention"
                                        .to_string(),
                                );
                            }
                        }
                        self.channel_states.update(channel_id, state);
                    } else {
                        // Show current settings
                        let mut lines = vec!["⚙️ **Channel Settings**\n".to_string()];
                        lines.push(format!(
                            "typing_indicator: {}",
                            if state.typing_indicator { "on" } else { "off" }
                        ));
                        lines.push(format!("max_history: {}", state.max_history));
                        lines.push(format!(
                            "require_mention: {}",
                            if state.require_mention { "on" } else { "off" }
                        ));
                        lines.push(format!("model: `{}`", state.model));
                        lines.push("\nUsage: `/settings key:<name> value:<value>`".to_string());
                        let _ = reply_tx.send(lines.join("\n"));
                    }
                }

                // ─── Help ───
                "help" => {
                    let help_text = r#"🦈 **OpenShark Discord Commands**

**Chat:**
• `/chat message:<text>` — Chat with OpenShark
• `/new` — Start fresh conversation
• `/system prompt:<text>` — Set custom system prompt
• `/reset` — Reset to defaults

**Models:**
• `/model` — List models
• `/model name:<model>` — Switch model
• `/models` — Detailed model list

**Tools:**
• `/tools` — List available tools
• `/tool name:<tool> args:<args>` — Execute a tool

**Agent:**
• `/agent task:<description>` — Run autonomous task

**Memory:**
• `/memory query:<text>` — Search memories
• `/remember fact:<text>` — Save a fact

**Info:**
• `/status` — Bot status
• `/stats` — Usage statistics
• `/settings` — View/change settings
• `/help` — This message
"#;
                    let _ = reply_tx.send(help_text.to_string());
                }

                _ => {
                    let _ = reply_tx.send(format!(
                        "❓ Unknown command: `{}`. Use `/help` for available commands.",
                        name
                    ));
                }
            }
        }

        Ok(())
    }

    /// Try to execute any TOOL: invocations in the response.
    async fn try_execute_tools(&self, response: &str) -> Option<String> {
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

/// Helper: extract a string option from slash command options.
fn get_string_option<'a>(
    options: &'a [serenity::all::CommandDataOption],
    name: &str,
) -> Option<&'a str> {
    options
        .iter()
        .find(|o| o.name == name)
        .and_then(|o| match &o.value {
            serenity::all::CommandDataOptionValue::String(s) => Some(s.as_str()),
            _ => None,
        })
}
