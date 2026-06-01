use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::providers::{ChatRequest, Message, Provider};
use crate::tools::{detect_tool_suggestions, AsyncToolExecutor};

use super::{AgentId, AgentStatus, SwarmAgent, SwarmEvent};

/// Context maintained per agent for conversation history.
pub struct AgentContext {
    pub messages: Vec<Message>,
    pub task_history: Vec<String>,
    pub tool_calls_count: usize,
}

impl AgentContext {
    pub fn new(system_prompt: &str) -> Self {
        Self {
            messages: vec![Message {
                role: "system".to_string(),
                content: system_prompt.to_string(),
                images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            }],
            task_history: Vec::new(),
            tool_calls_count: 0,
        }
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(Message {
            role: "user".to_string(),
            content: content.to_string(),
            images: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
        });
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(Message {
            role: "assistant".to_string(),
            content: content.to_string(),
            images: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
        });
    }

    pub fn add_tool_result(&mut self, tool_name: &str, result: &str) {
        self.messages.push(Message {
            role: "user".to_string(),
            content: format!("TOOL_RESULT:{}\n{}", tool_name, result),
            images: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
        });
    }

    /// Trim context to stay within model limits (simple sliding window).
    pub fn trim_context(&mut self, max_messages: usize) {
        if self.messages.len() <= max_messages {
            return;
        }
        // Always keep system message
        let system = self.messages.remove(0);
        let keep_count = max_messages - 1;
        let start = self.messages.len().saturating_sub(keep_count);
        let mut trimmed: Vec<Message> = self.messages.drain(start..).collect();
        trimmed.insert(0, system);
        self.messages = trimmed;
    }
}

/// Runner that executes a single agent's task loop with real LLM calls.
pub struct AgentRunner {
    agent_id: AgentId,
    provider: Provider,
    model: String,
    event_tx: mpsc::UnboundedSender<SwarmEvent>,
    context: Arc<RwLock<AgentContext>>,
    max_iterations: usize,
}

impl AgentRunner {
    pub fn new(
        agent_id: AgentId,
        provider: Provider,
        model: String,
        event_tx: mpsc::UnboundedSender<SwarmEvent>,
        system_prompt: &str,
    ) -> Self {
        Self {
            agent_id,
            provider,
            model,
            event_tx,
            context: Arc::new(RwLock::new(AgentContext::new(system_prompt))),
            max_iterations: 10,
        }
    }

    /// Execute a task: call LLM, optionally run tools, return result.
    pub async fn execute_task(
        &self,
        task: &str,
        agents: &Arc<RwLock<HashMap<AgentId, SwarmAgent>>>,
        agent_id: &AgentId,
    ) -> Result<String> {
        info!("🐝 Agent {} executing: {}", self.agent_id, task.chars().take(60).collect::<String>());

        // Update status to Working
        {
            let mut agents_lock = agents.write().await;
            if let Some(agent) = agents_lock.get_mut(agent_id) {
                agent.status = AgentStatus::Working {
                    task: task.to_string(),
                    started_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };
            }
        }

        // Broadcast activity start
        let _ = self.event_tx.send(SwarmEvent::AgentActivity {
            agent_id: self.agent_id.clone(),
            activity: format!("Starting: {}", task.chars().take(80).collect::<String>()),
        });

        let mut ctx = self.context.write().await;
        ctx.add_user_message(task);
        ctx.trim_context(20); // Keep last 20 messages + system
        let mut messages = ctx.messages.clone();
        drop(ctx);

        let mut final_result = String::new();

        for iteration in 0..self.max_iterations {
            debug!("🐝 Agent {} iteration {}/{}", self.agent_id, iteration + 1, self.max_iterations);

            let request = ChatRequest::new(self.model.clone(), messages.clone(), true);

            // Get agent name/role for chunk events
            let (agent_name, agent_role) = {
                let agents_lock = agents.read().await;
                if let Some(agent) = agents_lock.get(agent_id) {
                    (agent.name.clone(), agent.role.name.clone())
                } else {
                    (self.agent_id.clone(), "Unknown".to_string())
                }
            };

            let response = match tokio::time::timeout(
                std::time::Duration::from_secs(180), // 3 min — swarm agents run in parallel, requests queue
                self.provider.chat_stream(request)
            ).await {
                Ok(Ok((chunks, _metrics))) => {
                    let mut full_content = String::new();
                    for (i, chunk) in chunks.iter().enumerate() {
                        full_content.push_str(chunk);
                        // Filter persona-preamble from streaming chunks
                        let filtered_chunk = crate::swarm::persona_filter::strip_persona_preamble(chunk);
                        if !filtered_chunk.is_empty() {
                            let _ = self.event_tx.send(SwarmEvent::AgentChunk {
                                agent_id: self.agent_id.clone(),
                                agent_name: agent_name.clone(),
                                role: agent_role.clone(),
                                chunk: filtered_chunk,
                                is_final: i == chunks.len() - 1,
                            });
                        }
                    }
                    crate::swarm::persona_filter::strip_persona_preamble(&full_content)
                }
                Ok(Err(e)) => {
                    let err_msg = format!("LLM call failed: {}", e);
                    error!("🐝 Agent {}: {}", self.agent_id, err_msg);
                    let _ = self.event_tx.send(SwarmEvent::AgentError {
                        agent_id: self.agent_id.clone(),
                        error: err_msg.clone(),
                    });
                    {
                        let mut agents_lock = agents.write().await;
                        if let Some(agent) = agents_lock.get_mut(agent_id) {
                            agent.status = AgentStatus::Error { message: err_msg.clone() };
                            agent.errors_count += 1;
                        }
                    }
                    return Err(anyhow::anyhow!(err_msg));
                }
                Err(_) => {
                    let err_msg = "LLM call timed out after 180s".to_string();
                    error!("🐝 Agent {}: {}", self.agent_id, err_msg);
                    let _ = self.event_tx.send(SwarmEvent::AgentError {
                        agent_id: self.agent_id.clone(),
                        error: err_msg.clone(),
                    });
                    {
                        let mut agents_lock = agents.write().await;
                        if let Some(agent) = agents_lock.get_mut(agent_id) {
                            agent.status = AgentStatus::Error { message: err_msg.clone() };
                            agent.errors_count += 1;
                        }
                    }
                    return Err(anyhow::anyhow!(err_msg));
                }
            };

            let content = response;

            if content.is_empty() {
                warn!("🐝 Agent {} returned empty response", self.agent_id);
                break;
            }

            // Check for tool suggestions in the response
            let suggestions = detect_tool_suggestions(&content);
            if suggestions.is_empty() {
                // No tools needed — we're done
                final_result = content.clone();
                {
                    let mut ctx = self.context.write().await;
                    ctx.add_assistant_message(&content);
                }
                break;
            }

            // Execute the first suggested tool
            let suggestion = &suggestions[0];
            info!("🐝 Agent {} using tool: {}", self.agent_id, suggestion.tool_name);

            // Broadcast tool call
            let _ = self.event_tx.send(SwarmEvent::AgentToolCall {
                agent_id: self.agent_id.clone(),
                tool_name: suggestion.tool_name.clone(),
                args: suggestion.args.chars().take(100).collect::<String>(),
            });

            let executor = AsyncToolExecutor::new();
            let (tool_result, _success) = match executor.execute_with_timeout(
                suggestion.tool_name.clone(),
                suggestion.args.clone(),
                30000, // 30s timeout per tool call
            ).await {
                Ok((result, metrics)) => {
                    let success = metrics.success;
                    let formatted = if success {
                        format!("✅ {} ({}ms)\n{}", suggestion.tool_name, metrics.duration_ms, result)
                    } else {
                        format!("❌ {} ({}ms)\n{}", suggestion.tool_name, metrics.duration_ms, result)
                    };
                    let _ = self.event_tx.send(SwarmEvent::AgentToolResult {
                        agent_id: self.agent_id.clone(),
                        tool_name: suggestion.tool_name.clone(),
                        result: result.clone(),
                        success,
                    });
                    (formatted, success)
                }
                Err(e) => {
                    let err = format!("Tool execution error: {}", e);
                    let _ = self.event_tx.send(SwarmEvent::AgentToolResult {
                        agent_id: self.agent_id.clone(),
                        tool_name: suggestion.tool_name.clone(),
                        result: err.clone(),
                        success: false,
                    });
                    (err, false)
                }
            };

            {
                let mut ctx = self.context.write().await;
                ctx.add_assistant_message(&content);
                ctx.add_tool_result(&suggestion.tool_name, &tool_result);
                ctx.tool_calls_count += 1;
            }

            // Update messages for next iteration
            let ctx = self.context.read().await;
            messages = ctx.messages.clone();
            drop(ctx);

            final_result.push_str(&format!("\n\n[Tool: {}]\n{}", suggestion.tool_name, tool_result));
        }

        // Update agent status to completed
        {
            let mut agents_lock = agents.write().await;
            if let Some(agent) = agents_lock.get_mut(agent_id) {
                agent.status = AgentStatus::Completed { result: final_result.clone() };
                agent.cycles_completed += 1;
                agent.last_activity = Some(format!("Completed: {}", task));
            }
        }

        // Send work completed event
        let _ = self.event_tx.send(SwarmEvent::WorkCompleted {
            agent_id: self.agent_id.clone(),
            task: task.to_string(),
            result: final_result.clone(),
        });

        info!("🐝 Agent {} completed task ({} chars result)", self.agent_id, final_result.len());
        Ok(final_result)
    }

    /// Review another agent's work.
    pub async fn review_work(
        &self,
        target_agent: AgentId,
        work_content: &str,
        agent_ref: &Arc<RwLock<SwarmAgent>>,
    ) -> Result<(bool, String)> {
        info!("🐝 Agent {} reviewing {}", self.agent_id, target_agent);

        {
            let mut agent = agent_ref.write().await;
            agent.status = AgentStatus::Reviewing {
                target_agent: target_agent.clone(),
                started_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
        }

        let review_prompt = format!(
            "Review the following work from agent {}. Provide your assessment:\n\n{}\n\n\
             Respond with either:\n\
             APPROVED: <brief reason>\n\
             or\n\
             REJECTED: <specific feedback for improvement>",
            target_agent, work_content
        );

        let mut ctx = self.context.write().await;
        ctx.add_user_message(&review_prompt);
        ctx.trim_context(20);
        let messages = ctx.messages.clone();
        drop(ctx);

        let request = ChatRequest::new(self.model.clone(), messages, false);
        let response = self.provider.chat(request).await?;

        let content = response.choices.into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        let approved = content.to_uppercase().starts_with("APPROVED");
        let feedback = if approved {
            content.strip_prefix("APPROVED:").unwrap_or(&content).trim().to_string()
        } else {
            content.strip_prefix("REJECTED:").unwrap_or(&content).trim().to_string()
        };

        {
            let mut agent = agent_ref.write().await;
            agent.status = AgentStatus::Idle;
            agent.last_activity = Some(format!("Reviewed {}: {}", target_agent,
                if approved { "approved" } else { "rejected" }));
        }

        // Send review completed event
        let _ = self.event_tx.send(SwarmEvent::ReviewCompleted {
            reviewer: self.agent_id.clone(),
            target_agent: target_agent.clone(),
            approval: approved,
            feedback: feedback.clone(),
        });

        info!("🐝 Agent {} review of {}: {}", self.agent_id, target_agent,
            if approved { "APPROVED" } else { "REJECTED" });

        Ok((approved, feedback))
    }
}

/// Build a provider for a swarm agent from global config.
pub fn build_agent_provider(config: &Config) -> Result<Provider> {
    let (provider_name, provider_config) = config.find_provider_for_model(&config.default_model)
        .unwrap_or_else(|| config.providers.iter().next()
            .map(|(name, cfg)| (name.clone(), cfg.clone()))
            .unwrap_or_else(|| (
                "local".to_string(),
                crate::config::ProviderConfig {
                    base_url: "http://127.0.0.1:8080/v1".to_string(),
                    api_key: "local".to_string(),
                    models: vec![],
                    kind: crate::config::ProviderKind::OpenAiCompatible,
                    headers: std::collections::HashMap::new(),
                    env_file: None,
                }
            )));

    Ok(Provider::new(
        provider_name,
        provider_config.base_url,
        provider_config.api_key,
        provider_config.kind,
        provider_config.headers,
    ))
}
