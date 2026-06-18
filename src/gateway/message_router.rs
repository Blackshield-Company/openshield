use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::gateway::channel_state::{ChannelState, ChannelStateStore};
use crate::gateway::events::GatewayEvent;
use crate::gateway::session_branch::{BranchRegistry, diff_states};
use crate::memory::{ContextInjector, MemoryStore, Message as MemoryMessage};
use crate::providers::{ChatRequest, Message, Provider};
use crate::skills::{SkillRegistry, format_skills_prompt};
use crate::tools::{find_tool, get_tools};

/// Routes incoming gateway messages to the OpenShark engine.
pub struct MessageRouter {
    pub config: Config,
    memory: MemoryStore,
    channel_states: ChannelStateStore,
    skill_registry: Option<SkillRegistry>,
    branches: BranchRegistry,
}

impl MessageRouter {
    pub fn new(config: Config) -> Result<Self> {
        let memory = MemoryStore::new(&config.memory_db_path)?;
        let channel_states = ChannelStateStore::new(config.clone());
        let skills_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("openshark")
            .join("skills");
        let skill_registry = SkillRegistry::new(skills_dir).ok();

        Ok(Self {
            config,
            memory,
            channel_states,
            skill_registry,
            branches: BranchRegistry::new(),
        })
    }

    /// Handle a gateway event and stream the response back.
    pub async fn handle_event(&mut self, event: GatewayEvent) {
        match event {
            GatewayEvent::UserMessage {
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
            GatewayEvent::Ready => {
                info!("Gateway ready");
            }
            GatewayEvent::Disconnected => {
                warn!("Gateway disconnected");
            }
        }
    }

    /// Handle a Discord slash command interaction.
    #[cfg(feature = "discord")]
    pub async fn handle_discord_interaction(
        &mut self,
        interaction: serenity::all::Interaction,
        reply_tx: mpsc::UnboundedSender<String>,
    ) {
        if let Err(e) = self.handle_slash_command(interaction, reply_tx).await {
            error!("Failed to handle slash command: {}", e);
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

        // ── Keyword Detection ──────────────────────────────────────────────
        // Check for natural language memory queries and command keywords
        let content_lower = content.to_lowercase();
        let is_memory_query = content_lower.starts_with("what did we do about ")
            || content_lower.starts_with("how did we solve ")
            || content_lower.starts_with("tell me about ")
            || content_lower.starts_with("what was the issue with ")
            || content_lower.starts_with("remember when we ")
            || content_lower.starts_with("do you recall ");

        let is_command_keyword = content_lower.starts_with("!model ")
            || content_lower.starts_with("!tools")
            || content_lower.starts_with("!status")
            || content_lower.starts_with("!help")
            || content_lower.starts_with("!new")
            || content_lower.starts_with("!reset")
            || content_lower.starts_with("!branch")
            || content_lower.starts_with("!custom");

        // Handle command keywords directly
        if is_command_keyword {
            self.handle_keyword_command(&content_lower, channel_id, &reply_tx);
            return Ok(());
        }

        // Handle natural language memory queries
        if is_memory_query {
            let injector = ContextInjector::new(&self.memory);
            match injector.answer_natural_query(&content) {
                Ok(answer) => {
                    let _ = reply_tx.send(answer);
                }
                Err(e) => {
                    let _ = reply_tx.send(format!("❌ Memory query error: {}", e));
                }
            }
            return Ok(());
        }

        // ── Auto Memory Recall ─────────────────────────────────────────────
        // Search memory for relevant context and inject into system prompt
        let session_id = format!("discord-{}", channel_id);
        let relevant_context = self.fetch_relevant_memory(&session_id, &content);

        // ── Dynamic Skill Injection ────────────────────────────────────────
        // Check if any skills are triggered by the user's query
        let triggered_skills = self
            .skill_registry
            .as_ref()
            .map(|r| r.find_triggered(&content))
            .unwrap_or_default();
        let skills_prompt = if !triggered_skills.is_empty() {
            let skill_refs: Vec<_> = triggered_skills.to_vec();
            format_skills_prompt(&skill_refs)
        } else {
            String::new()
        };

        // Build messages with context injection
        let mut messages = state.get_messages();

        // If we found relevant memory, inject it as a system context message
        if !relevant_context.is_empty() {
            let context_msg = Message {
                role: "system".to_string(),
                content: format!(
                    "[RELEVANT CONTEXT FROM MEMORY]\n{}\n[END CONTEXT]",
                    relevant_context
                ),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            };
            // Insert after the first system message
            if messages.len() > 1 {
                messages.insert(1, context_msg);
            } else {
                messages.push(context_msg);
            }
        }

        // If skills were triggered, inject them as a system message
        if !skills_prompt.is_empty() {
            let skills_msg = Message {
                role: "system".to_string(),
                content: skills_prompt,
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            };
            // Insert after system prompt (and after memory context if present)
            let insert_pos = if messages.len() > 1 && messages[1].role == "system" {
                2
            } else {
                1
            };
            if insert_pos <= messages.len() {
                messages.insert(insert_pos, skills_msg);
            } else {
                messages.push(skills_msg);
            }
        }

        // Add user message
        state.add_user_message(format!("{}: {}", username, content));
        messages.push(Message {
            role: "user".to_string(),
            content: format!("{}: {}", username, content),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        // Persist to memory
        let _ = self.memory.save_message(&MemoryMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            role: "user".to_string(),
            content: format!("{}: {}", username, content),
            created_at: chrono::Utc::now(),
            tokens_used: None,
        });

        // Stream response
        let req = ChatRequest::new(state.model.clone(), messages.clone(), true);
        let provider = state.provider.clone();

        // ── Multi-Model Mode (Optional) ────────────────────────────────────
        if state.multi_model_enabled && !state.multi_model_secondary.is_empty() {
            self.handle_multi_model_response(channel_id, &state, &messages, &reply_tx)
                .await;
        } else {
            // Standard single-model response
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
        }

        // Save updated state
        self.channel_states.update(channel_id, state);

        Ok(())
    }

    /// Fetch relevant memory context for a query.
    fn fetch_relevant_memory(&self, _session_id: &str, query: &str) -> String {
        let _injector = ContextInjector::new(&self.memory);

        // Try semantic search first
        match self.memory.semantic_search(query, 3) {
            Ok(results) if !results.is_empty() => {
                let mut context = String::new();
                for (msg, score) in results.iter().take(3) {
                    if *score > 0.3 {
                        context.push_str(&format!(
                            "[{}] {}: {}\n",
                            msg.created_at.format("%Y-%m-%d"),
                            msg.role,
                            &msg.content[..msg.content.len().min(150)]
                        ));
                    }
                }
                context
            }
            _ => {
                // Fallback: keyword search
                match self.memory.search_messages(query, 3) {
                    Ok(messages) if !messages.is_empty() => messages
                        .iter()
                        .map(|msg| {
                            format!(
                                "[{}] {}: {}",
                                msg.created_at.format("%Y-%m-%d"),
                                msg.role,
                                &msg.content[..msg.content.len().min(150)]
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                }
            }
        }
    }

    /// Handle branch commands (!branch save|load|list|delete|diff).
    fn handle_branch_command(
        &self,
        arg: &str,
        channel_id: u64,
        reply_tx: &mpsc::UnboundedSender<String>,
    ) {
        let parts: Vec<&str> = arg.splitn(2, ' ').collect();
        let subcmd = parts.first().copied().unwrap_or("").trim();
        let subarg = parts.get(1).copied().unwrap_or("").trim();

        match subcmd {
            "save" => {
                if subarg.is_empty() {
                    let _ = reply_tx.send("❌ Usage: `!branch save <name>`".to_string());
                    return;
                }
                let state = self.channel_states.get_or_create(channel_id);
                self.branches.save(channel_id, subarg, state);
                let _ = reply_tx.send(format!(
                    "🌿 Branch `{}` saved ({} messages).",
                    subarg,
                    self.branches
                        .list(channel_id)
                        .iter()
                        .find(|b| b.name == subarg)
                        .map(|b| b.message_count)
                        .unwrap_or(0)
                ));
            }
            "load" => {
                if subarg.is_empty() {
                    let _ = reply_tx.send("❌ Usage: `!branch load <name>`".to_string());
                    return;
                }
                if let Some(state) = self.branches.load(channel_id, subarg) {
                    self.channel_states.update(channel_id, state);
                    let _ = reply_tx.send(format!(
                        "🌿 Branch `{}` loaded. Current state restored.",
                        subarg
                    ));
                } else {
                    let _ = reply_tx.send(format!("❌ Branch `{}` not found.", subarg));
                }
            }
            "list" => {
                let branches = self.branches.list(channel_id);
                if branches.is_empty() {
                    let _ = reply_tx.send(
                        "🌿 No branches saved for this channel.\n\nUsage: `!branch save <name>`"
                            .to_string(),
                    );
                } else {
                    let mut lines = vec!["🌿 **Branches**\n".to_string()];
                    for b in branches {
                        lines.push(format!(
                            "• `{}` | {} messages | model: `{}` | {}",
                            b.name,
                            b.message_count,
                            b.model,
                            b.created_at.format("%Y-%m-%d %H:%M")
                        ));
                    }
                    let _ = reply_tx.send(lines.join("\n"));
                }
            }
            "delete" => {
                if subarg.is_empty() {
                    let _ = reply_tx.send("❌ Usage: `!branch delete <name>`".to_string());
                    return;
                }
                if self.branches.delete(channel_id, subarg) {
                    let _ = reply_tx.send(format!("🌿 Branch `{}` deleted.", subarg));
                } else {
                    let _ = reply_tx.send(format!("❌ Branch `{}` not found.", subarg));
                }
            }
            "diff" => {
                if subarg.is_empty() {
                    let _ = reply_tx.send("❌ Usage: `!branch diff <name>`".to_string());
                    return;
                }
                let current = self.channel_states.get_or_create(channel_id);
                if let Some(branch) = self.branches.load(channel_id, subarg) {
                    let diff = diff_states(&current, &branch);
                    let _ = reply_tx.send(format!("🌿 **Diff vs `{}`**\n{}", subarg, diff));
                } else {
                    let _ = reply_tx.send(format!("❌ Branch `{}` not found.", subarg));
                }
            }
            _ => {
                let help = r#"🌿 **Branch Commands**

• `!branch save <name>` — Save current conversation state
• `!branch load <name>` — Restore a saved branch
• `!branch list` — Show all saved branches
• `!branch delete <name>` — Delete a branch
• `!branch diff <name>` — Compare current state to branch

Branches are per-channel and last until restart."#;
                let _ = reply_tx.send(help.to_string());
            }
        }
    }
    fn handle_keyword_command(
        &self,
        content: &str,
        channel_id: u64,
        reply_tx: &mpsc::UnboundedSender<String>,
    ) {
        let parts: Vec<&str> = content.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).unwrap_or(&"").trim();

        match cmd {
            "!model" => {
                if arg.is_empty() {
                    let state = self.channel_states.get_or_create(channel_id);
                    let mut lines = vec![format!("Current model: `{}`\n", state.model)];
                    for (provider_name, provider) in &self.config.providers {
                        for m in &provider.models {
                            let marker = if m.name == state.model { "●" } else { "○" };
                            lines.push(format!(
                                "{} `{}` | ctx={} | {}",
                                marker, m.name, m.context_length, provider_name
                            ));
                        }
                    }
                    let _ = reply_tx.send(lines.join("\n"));
                } else {
                    let mut state = self.channel_states.get_or_create(channel_id);
                    match state.switch_model(arg, &self.config) {
                        Ok(()) => {
                            self.channel_states.update(channel_id, state);
                            let _ = reply_tx.send(format!("🤖 Model switched to: `{}`", arg));
                        }
                        Err(e) => {
                            let _ = reply_tx.send(format!("❌ {}", e));
                        }
                    }
                }
            }
            "!tools" => {
                let tools = get_tools();
                let mut lines = vec!["🔧 **Available Tools**\n".to_string()];
                for tool in tools {
                    lines.push(format!("• `{}`: {}", tool.name(), tool.description()));
                }
                let _ = reply_tx.send(lines.join("\n"));
            }
            "!status" => {
                let state = self.channel_states.get_or_create(channel_id);
                let mut lines = vec!["🦈 **OpenShark Status**\n".to_string()];
                lines.push(format!("Model: `{}`", state.model));
                lines.push(format!(
                    "History: {} messages",
                    state.history.len().saturating_sub(1)
                ));
                lines.push(format!("Version: {}", self.config.version));
                let _ = reply_tx.send(lines.join("\n"));
            }
            "!help" => {
                let help = r#"🦈 **OpenShark Keyword Commands**

|**Prefix commands:**
• `!model` — List models
• `!model <name>` — Switch model
• `!tools` — List available tools
• `!status` — Show status
• `!new` — Start fresh conversation
• `!reset` — Reset to defaults
• `!multi` — Toggle multi-model mode
• `!multi on/off` — Enable/disable multi-model
• `!multi <model1, model2>` — Set secondary models
• `!help` — This message

|**Natural language queries:**
• "What did we do about X?"
• "How did we solve X?"
• "Tell me about X"
• "What was the issue with X?"
• "Remember when we..."

|**Slash commands:**
Use `/help` for the full slash command list.
"#;
                let _ = reply_tx.send(help.to_string());
            }
            "!new" => {
                self.channel_states.remove(channel_id);
                let _ = reply_tx.send("🆕 Fresh conversation started.".to_string());
            }
            "!reset" => {
                let mut state = self.channel_states.get_or_create(channel_id);
                state.reset(&self.config);
                self.channel_states.update(channel_id, state);
                let _ = reply_tx.send("🔄 Reset complete.".to_string());
            }
            "!multi" => {
                let mut state = self.channel_states.get_or_create(channel_id);
                if arg.is_empty() {
                    // Toggle multi-model mode
                    state.multi_model_enabled = !state.multi_model_enabled;
                    let status = if state.multi_model_enabled {
                        "✅ ON"
                    } else {
                        "❌ OFF"
                    };
                    let _ = reply_tx.send(format!(
                        "🤖 Multi-model mode: {}\n\nSecondary models: {}",
                        status,
                        if state.multi_model_secondary.is_empty() {
                            "none configured".to_string()
                        } else {
                            state.multi_model_secondary.join(", ")
                        }
                    ));
                } else if arg == "on" {
                    state.multi_model_enabled = true;
                    let _ = reply_tx.send("🤖 Multi-model mode: ✅ ON".to_string());
                } else if arg == "off" {
                    state.multi_model_enabled = false;
                    let _ = reply_tx.send("🤖 Multi-model mode: ❌ OFF".to_string());
                } else {
                    // Set secondary models
                    let models: Vec<String> =
                        arg.split(',').map(|s| s.trim().to_string()).collect();
                    state.multi_model_secondary = models.clone();
                    let _ = reply_tx.send(format!(
                        "🤖 Secondary models set to: {}\nUse `!multi on` to enable.",
                        models.join(", ")
                    ));
                }
                self.channel_states.update(channel_id, state);
            }
            "!branch" => {
                self.handle_branch_command(arg, channel_id, reply_tx);
            }
            "!custom" => {
                let custom_tools = crate::tools::custom::get_custom_tools();
                if custom_tools.is_empty() {
                    let _ = reply_tx.send(
                        "🔧 No custom tools configured.\n\nCreate `~/.config/openshark/custom_tools.toml`:\n\n```toml\n[[tool]]\nname = \"weather\"\ndescription = \"Get weather for a city\"\ncommand = \"curl -s 'wttr.in/{{args}}?format=3'\"\n```"
                            .to_string(),
                    );
                } else {
                    let mut lines = vec!["🔧 **Custom Tools**\n".to_string()];
                    for tool in custom_tools {
                        lines.push(format!("• `{}`: {}", tool.name(), tool.description()));
                    }
                    lines.push("\nUse `!tool <name> <args>` or `/tool` to execute.".to_string());
                    let _ = reply_tx.send(lines.join("\n"));
                }
            }
            _ => {
                let _ = reply_tx.send(format!("❓ Unknown command: `{}`. Try `!help`", cmd));
            }
        }
    }

    #[cfg(feature = "discord")]
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
                    let content =
                        get_string_option(&cmd.data.options, "message").unwrap_or("Hello!");
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
                    let _ = reply_tx
                        .send("🆕 Fresh conversation started. History cleared.".to_string());
                }

                "system" => {
                    if let Some(prompt) = get_string_option(&cmd.data.options, "prompt") {
                        let mut state = self.channel_states.get_or_create(channel_id);
                        state.set_system_prompt(prompt);
                        self.channel_states.update(channel_id, state);
                        let _ =
                            reply_tx.send("📝 System prompt updated for this channel.".to_string());
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
                                let _ = reply_tx
                                    .send(format!("🤖 Model switched to: **{}**", model_name));
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
                                let marker = if m.name == current_model {
                                    "●"
                                } else {
                                    "○"
                                };
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
                            let default_marker = if model.name == self.config.default_model {
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
                        let _ = reply_tx.send(format!(
                            "🦈 Starting agent task: **{}**\nThis may take a moment...",
                            task
                        ));

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
                    lines.push(
                        "\nUse `/tool name:<tool> args:<arguments>` to execute directly."
                            .to_string(),
                    );
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
                        let _ = reply_tx.send(format!("🔧 Executing `{} {}`...", tool_name, args));
                        match tool.execute(args) {
                            Ok(result) => {
                                let display = if result.len() > 1800 {
                                    format!(
                                        "{}\n\n... (truncated)",
                                        crate::utils::truncate_str(&result, 1800)
                                    )
                                } else {
                                    result
                                };
                                let _ =
                                    reply_tx.send(format!("✅ **Result:**\n```\n{}\n```", display));
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
                                    let _ = reply_tx
                                        .send("🔍 No memories found for that query.".to_string());
                                } else {
                                    let mut lines = vec![format!(
                                        "🔍 **Memory Search:** '{}' ({} results)\n",
                                        query,
                                        messages.len()
                                    )];
                                    for msg in messages {
                                        let preview = if msg.content.len() > 200 {
                                            format!(
                                                "{}...",
                                                crate::utils::truncate_str(&msg.content, 200)
                                            )
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
                        let _ = reply_tx
                            .send("❌ Usage: `/memory query:your search query`".to_string());
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
                                let _ =
                                    reply_tx.send("💾 Fact saved to long-term memory.".to_string());
                            }
                            Err(e) => {
                                let _ = reply_tx.send(format!("❌ Failed to save: {}", e));
                            }
                        }
                    } else {
                        let _ = reply_tx
                            .send("❌ Usage: `/remember fact:the fact to remember`".to_string());
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

                "stats" => match (
                    self.memory.get_stats_summary(),
                    self.memory.get_model_usage_stats(),
                    self.memory.get_tool_usage_stats(),
                    self.memory.get_daily_activity(7),
                ) {
                    (Ok(stats), Ok(model_stats), Ok(tool_stats), Ok(activity)) => {
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

                        if !model_stats.is_empty() {
                            lines.push("\n**Model Breakdown**".to_string());
                            for m in model_stats.iter().take(5) {
                                lines.push(format!(
                                    "• `{}` | {} sessions | {} msgs | {} tokens | {:.0}% tools",
                                    m.model,
                                    m.session_count,
                                    m.message_count,
                                    m.total_tokens,
                                    m.tool_success_rate
                                ));
                            }
                        }

                        if !tool_stats.is_empty() {
                            lines.push("\n**Tool Breakdown**".to_string());
                            for t in tool_stats.iter().take(5) {
                                lines.push(format!(
                                    "• `{}` | {} calls | {} success | {:.0}%",
                                    t.tool_name, t.total_calls, t.successful_calls, t.success_rate
                                ));
                            }
                        }

                        if !activity.is_empty() {
                            lines.push("\n**Last 7 Days**".to_string());
                            for day in activity.iter().take(7) {
                                lines.push(format!(
                                    "• {} | {} sessions | {} models",
                                    day.day, day.session_count, day.model_count
                                ));
                            }
                        }

                        let _ = reply_tx.send(lines.join("\n"));
                    }
                    _ => {
                        let _ = reply_tx.send("❌ Error loading stats.".to_string());
                    }
                },

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

                // ─── Multi-Model ───
                "multi" => {
                    let mut state = self.channel_states.get_or_create(channel_id);
                    let action = get_string_option(&cmd.data.options, "action");
                    let models = get_string_option(&cmd.data.options, "models");

                    if let Some(action) = action {
                        match action {
                            "on" => {
                                state.multi_model_enabled = true;
                                let _ = reply_tx.send("🤖 Multi-model mode: ✅ ON".to_string());
                            }
                            "off" => {
                                state.multi_model_enabled = false;
                                let _ = reply_tx.send("🤖 Multi-model mode: ❌ OFF".to_string());
                            }
                            "toggle" => {
                                state.multi_model_enabled = !state.multi_model_enabled;
                                let status = if state.multi_model_enabled {
                                    "✅ ON"
                                } else {
                                    "❌ OFF"
                                };
                                let _ = reply_tx.send(format!("🤖 Multi-model mode: {}", status));
                            }
                            "set" => {
                                if let Some(models_str) = models {
                                    let model_list: Vec<String> = models_str
                                        .split(',')
                                        .map(|s| s.trim().to_string())
                                        .collect();
                                    state.multi_model_secondary = model_list.clone();
                                    let _ = reply_tx.send(format!(
                                        "🤖 Secondary models set to: {}\nUse `/multi action:on` to enable.",
                                        model_list.join(", ")
                                    ));
                                } else {
                                    let _ = reply_tx.send(
                                        "❌ Please provide models. Usage: `/multi action:set models:model1,model2`".to_string(),
                                    );
                                }
                            }
                            _ => {
                                let _ = reply_tx.send(
                                    "❌ Unknown action. Use: on, off, toggle, set".to_string(),
                                );
                            }
                        }
                    } else {
                        // Show current status
                        let status = if state.multi_model_enabled {
                            "✅ ON"
                        } else {
                            "❌ OFF"
                        };
                        let secondary = if state.multi_model_secondary.is_empty() {
                            "none configured".to_string()
                        } else {
                            state.multi_model_secondary.join(", ")
                        };
                        let _ = reply_tx.send(format!(
                            "🤖 Multi-model mode: {}\nSecondary models: {}\n\nUsage: `/multi action:on/off/toggle/set models:model1,model2`",
                            status, secondary
                        ));
                    }
                    self.channel_states.update(channel_id, state);
                }

                // ─── Help ───
                "help" => {
                    let help_text = r#"🦈 **OpenShark Discord Commands**

**Chat:**
• `/chat message:<text>` — Chat with OpenShark
• `/new` — Start fresh conversation
• `/system prompt:<text>` — Set custom system prompt
• `/reset` — Reset to defaults

|**Models:**
• `/model` — List models
• `/model name:<model>` — Switch model
• `/models` — Detailed model list
• `/multi` — Multi-model mode control

|**Tools:**
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

**Keyword commands (no slash needed):**
• `!model`, `!tools`, `!status`, `!help`, `!new`, `!reset`

**Natural language memory:**
• "What did we do about X?"
• "How did we solve X?"
• "Tell me about X"
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

    /// Handle multi-model response: query primary + secondary models, format comparison.
    async fn handle_multi_model_response(
        &self,
        _channel_id: u64,
        state: &ChannelState,
        messages: &[Message],
        reply_tx: &mpsc::UnboundedSender<String>,
    ) {
        let primary_model = state.model.clone();
        let secondary_models = state.multi_model_secondary.clone();

        // Query primary model
        let primary_req = ChatRequest::new(primary_model.clone(), messages.to_vec(), true);
        let primary_provider = state.provider.clone();

        let primary_response = match primary_provider.chat_stream(primary_req).await {
            Ok((chunks, _)) => chunks.join(""),
            Err(e) => {
                let _ = reply_tx.send(format!("❌ Primary model error: {}", e));
                return;
            }
        };

        // Query secondary models in parallel
        let mut secondary_responses: Vec<(String, String)> = Vec::new();

        for model_name in &secondary_models {
            if let Some((provider_name, provider_cfg)) =
                self.config.find_provider_for_model(model_name)
            {
                let provider = Provider::new(
                    provider_name.clone(),
                    provider_cfg.base_url.clone(),
                    provider_cfg.api_key.clone(),
                    provider_cfg.kind.clone(),
                    provider_cfg.headers.clone(),
                );

                let req = ChatRequest::new(model_name.clone(), messages.to_vec(), true);

                match provider.chat_stream(req).await {
                    Ok((chunks, _)) => {
                        let response = chunks.join("");
                        secondary_responses.push((model_name.clone(), response));
                    }
                    Err(e) => {
                        secondary_responses.push((model_name.clone(), format!("❌ Error: {}", e)));
                    }
                }
            } else {
                secondary_responses.push((
                    model_name.clone(),
                    "❌ Model not found in config".to_string(),
                ));
            }
        }

        // Format comparison response
        let mut output = format!("🤖 **Primary** (`{}`)\n{}", primary_model, primary_response);

        if !secondary_responses.is_empty() {
            output.push_str("\n\n---\n\n");
            for (model_name, response) in &secondary_responses {
                output.push_str(&format!("🤖 **{}**\n{}\n\n", model_name, response));
            }
        }

        let _ = reply_tx.send(output);
    }

    /// Try to execute any TOOL: or TOOL. invocations in the response.
    async fn try_execute_tools(&self, response: &str) -> Option<String> {
        // Find first occurrence of either TOOL: or TOOL.
        let tool_start = response.find("TOOL:").or_else(|| response.find("TOOL."))?;
        let tool_line = &response[tool_start..];
        let tool_line = tool_line.lines().next()?;
        // Strip prefix (either "TOOL:" or "TOOL.")
        let rest = &tool_line[5..];
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if parts.is_empty() {
            return None;
        }

        let tool_name = parts[0].trim();
        let args = parts.get(1).unwrap_or(&"").trim();

        // Security gate — apply security checks before executing any tool
        let security = match crate::security::SecurityEngine::new(
            crate::security::SecurityConfig::load().unwrap_or_default()
        ) {
            Ok(s) => s,
            Err(e) => {
                return Some(format!("🔒 Security engine failed for tool '{}': {}", tool_name, e));
            }
        };

        match security.check_tool_call(tool_name, args) {
            crate::security::SecurityDecision::Allow => {}
            crate::security::SecurityDecision::RequireApproval { reason, .. } => {
                return Some(format!("🔒 Tool '{}' requires approval: {}", tool_name, reason));
            }
            crate::security::SecurityDecision::Deny { reason } => {
                return Some(format!("🚫 Tool '{}' blocked: {}", tool_name, reason));
            }
        }

        if let Some(tool) = find_tool(tool_name) {
            match tool.execute(args) {
                Ok(result) => Some(result),
                Err(e) => Some(format!("Tool error: {}", e)),
            }
        } else {
            Some(format!("Unknown tool: {}", tool_name))
        }
    }
}

/// Helper: extract a string option from slash command options.
#[cfg(feature = "discord")]
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
