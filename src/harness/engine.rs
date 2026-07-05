//! OpenShark AI Harness — Core Engine
//!
//! The `HarnessEngine` is the central orchestrator for OpenShark's AI interactions.
//! It manages conversation state, tool calling loops, memory injection, skill triggering,
//! multi-model queries, and security gating.
//!
//! ## Usage
//!
//! ```rust,ignore
//! let engine = HarnessEngine::new(config, memory, provider).await?;
//! let response = engine.run_turn("Fix the bug in src/main.rs").await?;
//! // response.primary.content has the model's text response
//! // response.tool_results has any executed tool results
//! // response.secondary has alternative model responses
//! ```

use anyhow::{Context, Result};
use chrono::Utc;
use std::time::Instant;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::Config;
use crate::memory::{MemoryStore, Message as MemoryMessage, ToolCall as MemoryToolCall};
use crate::providers::{
    AccumulatedToolCall, ChatRequest, Message, Provider, StreamChunk, StreamMetrics, ToolCallRequest,
};
use crate::security::{SecurityDecision, SecurityEngine};
use crate::skills::{SkillRegistry, format_skills_prompt};
use crate::tools::{execute_tool, get_openai_tool_definitions, normalize_tool_args};

use super::event::HarnessEvent;
use super::response::{HarnessResponse, HarnessState, ModelResponse, ToolExecutionResult};

/// Configuration for the harness engine.
#[derive(Debug, Clone)]
pub struct HarnessConfig {
    /// Maximum tool calling loops per turn.
    pub max_tool_loops: usize,
    /// Whether to require user approval for tool calls.
    pub require_tool_approval: bool,
    /// Whether to enable multi-model responses.
    pub multi_model_enabled: bool,
    /// Number of past messages to inject from memory.
    pub memory_context_limit: usize,
    /// Whether to include skills in the system prompt.
    pub skills_enabled: bool,
    /// Model to use for the primary response.
    pub primary_model: String,
    /// Secondary models for comparison.
    pub secondary_models: Vec<String>,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            max_tool_loops: 10,
            require_tool_approval: false,
            multi_model_enabled: false,
            memory_context_limit: 5,
            skills_enabled: true,
            primary_model: "kimi-k2.6".to_string(),
            secondary_models: Vec::new(),
        }
    }
}

/// The central AI harness engine.
impl HarnessEngine {
    /// Build a static system prompt used for tests and headless mode.
    #[allow(dead_code)]
    pub fn build_system_prompt_static() -> String {
        let mut prompt = String::from("You are OpenShark, an autonomous AI coding agent.\n\n");
        prompt.push_str("## AVAILABLE TOOLS\n");
        prompt.push_str("You have access to tools. When you need to use a tool, respond with a tool call. ");
        prompt.push_str("The system will execute the tool and return the result to you.\n");
        prompt.push_str("## MEMORY\n");
        prompt.push_str("You have access to persistent memory across sessions.\n");
        prompt
    }
}

pub struct HarnessEngine {
    config: HarnessConfig,
    app_config: Config,
    memory: MemoryStore,
    primary_provider: Provider,
    secondary_providers: Vec<(String, Provider)>,
    security_engine: SecurityEngine,
    skill_registry: Option<SkillRegistry>,
    state: HarnessState,
    session_id: String,
}

#[allow(dead_code)]
impl HarnessEngine {
    /// Create a new harness engine.
    pub fn new(
        harness_config: HarnessConfig,
        app_config: Config,
        memory: MemoryStore,
    ) -> Result<Self> {
        // Initialize primary provider
        let (provider_name, provider_config) = app_config
            .find_provider_for_model(&harness_config.primary_model)
            .unwrap_or_else(|| {
                let (n, p) = app_config.providers.iter().next().unwrap_or_else(|| {
                    panic!("No providers configured in config");
                });
                (n.clone(), p.clone())
            });

        let _primary_provider = Provider::new(
            provider_name,
            provider_config.base_url.clone(),
            provider_config.api_key.clone(),
            provider_config.kind.clone(),
            provider_config.headers.clone(),
        );

        let security_engine = SecurityEngine::new(
            crate::security::SecurityConfig::load().unwrap_or_default(),
        )?;

        Self::new_with_security(harness_config, app_config, memory, security_engine)
    }

    /// Create a harness engine with a pre-configured security engine.
    pub fn new_with_security(
        harness_config: HarnessConfig,
        app_config: Config,
        memory: MemoryStore,
        security_engine: SecurityEngine,
    ) -> Result<Self> {
        let session_id = Uuid::new_v4().to_string();

        // Initialize primary provider
        let (provider_name, provider_config) = app_config
            .find_provider_for_model(&harness_config.primary_model)
            .unwrap_or_else(|| {
                let (n, p) = app_config.providers.iter().next().unwrap_or_else(|| {
                    panic!("No providers configured in config");
                });
                (n.clone(), p.clone())
            });

        let primary_provider = Provider::new(
            provider_name,
            provider_config.base_url.clone(),
            provider_config.api_key.clone(),
            provider_config.kind.clone(),
            provider_config.headers.clone(),
        );

        // Initialize secondary providers
        let mut secondary_providers = Vec::new();
        for model in &harness_config.secondary_models {
            if let Some((name, cfg)) = app_config.find_provider_for_model(model) {
                let provider = Provider::new(
                    name.clone(),
                    cfg.base_url.clone(),
                    cfg.api_key.clone(),
                    cfg.kind.clone(),
                    cfg.headers.clone(),
                );
                secondary_providers.push((model.clone(), provider));
            }
        }

        // Load skill registry
        let skills_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("openshark")
            .join("skills");
        let skill_registry = SkillRegistry::new(skills_dir).ok();

        memory.create_session(&session_id, &harness_config.primary_model, "harness")?;

        Ok(Self {
            config: harness_config,
            app_config,
            memory,
            primary_provider,
            secondary_providers,
            security_engine,
            skill_registry,
            state: HarnessState::new(session_id.clone()),
            session_id,
        })
    }

    /// Create a harness engine for an existing session with an initial conversation history.
    /// Used by the TUI so it can share a session while running the engine in a spawned task.
    pub fn new_with_history(
        harness_config: HarnessConfig,
        app_config: Config,
        memory: MemoryStore,
        session_id: String,
        initial_messages: Vec<Message>,
    ) -> Result<Self> {
        Self::new_with_history_and_security(
            harness_config,
            app_config,
            memory,
            session_id,
            initial_messages,
            SecurityEngine::new(crate::security::SecurityConfig::load().unwrap_or_default())?,
        )
    }

    pub fn new_with_history_and_security(
        harness_config: HarnessConfig,
        app_config: Config,
        memory: MemoryStore,
        session_id: String,
        initial_messages: Vec<Message>,
        security_engine: SecurityEngine,
    ) -> Result<Self> {
        // Ensure the session exists in this memory store.
        let _ = memory.create_session(&session_id, &harness_config.primary_model, "harness");

        let mut engine = Self::new_with_security(harness_config, app_config, memory, security_engine)?;
        engine.session_id = session_id.clone();
        engine.state = HarnessState::new(session_id);
        engine.state.messages = initial_messages;
        Ok(engine)
    }
    /// Run a single turn of the harness loop.
    /// This handles: memory injection → skill triggering → model call → tool execution → response.
    pub async fn run_turn(
        &mut self,
        user_message: &str,
    ) -> Result<HarnessResponse> {
        self.state.turn_count += 1;

        // Build the conversation messages
        let mut messages = self.build_conversation_messages(user_message)?;

        // Get tool definitions
        let tools = get_openai_tool_definitions();

        // Create the chat request
        let request = ChatRequest {
            model: self.config.primary_model.clone(),
            messages: messages.clone(),
            stream: false,
            max_tokens: None,
            temperature: None,
            tools: Some(tools.clone()),
        };

        // Query primary model
        let primary_response = self.query_primary(request).await?;

        // Handle tool calls if present
        let mut tool_results = Vec::new();
        let mut had_tool_calls = false;

        if !primary_response.tool_calls.is_empty() {
            had_tool_calls = true;

            // Execute tool calls with loop support
            let mut loop_count = 0;
            let mut current_messages = messages.clone();
            let mut current_tool_calls = primary_response.tool_calls.clone();
            let mut current_content = primary_response.content.clone();
            let mut current_reasoning = primary_response.reasoning.clone();

            // Add the assistant message with tool_calls
            current_messages.push(Message {
                role: "assistant".to_string(),
                content: current_content.clone(),
                images: None,
                tool_call_id: None,
                tool_calls: Some(current_tool_calls.clone()),
                reasoning_content: current_reasoning.clone(),
            });

            while loop_count < self.config.max_tool_loops {
                loop_count += 1;

                // Execute all tool calls in this response
                let mut batch_results = Vec::new();
                for tool_call in &current_tool_calls {
                    let result = self.execute_tool_call(tool_call).await?;
                    batch_results.push(result);
                }

                // Add tool results to messages
                for result in &batch_results {
                    current_messages.push(Message {
                        role: "tool".to_string(),
                        content: result.result.clone(),
                        images: None,
                        tool_call_id: Some(result.tool_call_id.clone()),
                        tool_calls: None,
                        reasoning_content: None,
                    });
                }

                tool_results.extend(batch_results);

                // Check if we need another loop
                let follow_up_request = ChatRequest {
                    model: self.config.primary_model.clone(),
                    messages: current_messages.clone(),
                    stream: false,
                    max_tokens: None,
                    temperature: None,
                    tools: Some(tools.clone()),
                };

                let follow_up = self.primary_provider.chat(follow_up_request).await?;
                let choice = follow_up.choices.first().context("No response from model")?;

                current_content = choice.message.content.clone();
                current_reasoning = choice.message.reasoning_content.clone();

                // Update messages with the follow-up
                current_messages.push(Message {
                    role: "assistant".to_string(),
                    content: current_content.clone(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: choice.message.tool_calls.clone(),
                    reasoning_content: current_reasoning.clone(),
                });

                // If no more tool calls, we're done
                if choice.message.tool_calls.is_none() || choice.message.tool_calls.as_ref().unwrap().is_empty() {
                    break;
                }

                // Prepare for next loop iteration
                current_tool_calls = choice.message.tool_calls.clone().unwrap();
            }

            // Update state messages with the full conversation
            messages = current_messages;
        } else {
            // No tool calls, just add the assistant response
            messages.push(Message {
                role: "assistant".to_string(),
                content: primary_response.content.clone(),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: primary_response.reasoning.clone(),
            });
        }

        // Save to memory
        self.save_turn_to_memory(user_message, &messages, &tool_results)?;

        // Update state
        self.state.messages = messages;
        self.state.tool_results_history.extend(tool_results.clone());

        // Query secondary models if enabled
        let secondary = if self.config.multi_model_enabled {
            self.query_secondary_models(user_message).await?
        } else {
            Vec::new()
        };

        // Calculate totals
        let total_tokens = primary_response.metrics.tokens_generated as u64;
        let total_cost = 0.0; // TODO: track from usage

        self.state.total_tokens += total_tokens;
        self.state.total_cost_usd += total_cost;

        Ok(HarnessResponse {
            primary: primary_response,
            secondary,
            tool_results,
            had_tool_calls,
            total_tokens,
            total_cost_usd: total_cost,
        })
    }

    /// Run a single turn of the harness loop and emit streaming events via `tx`.
    /// The caller is responsible for applying the events to the UI/state.
    ///
    /// When `persist` is `true`, the turn is saved to the harness's memory store.
    /// The TUI typically sets this to `false` because it already saves messages in the main thread.
    pub async fn run_turn_streaming(
        &mut self,
        user_message: &str,
        tx: mpsc::UnboundedSender<HarnessEvent>,
        persist: bool,
    ) -> Result<HarnessResponse> {
        self.state.turn_count += 1;
        let _ = tx.send(HarnessEvent::Start);

        // Build the conversation messages
        let mut messages = self.build_conversation_messages(user_message)?;

        // Get tool definitions
        let tools = get_openai_tool_definitions();

        // Query primary model via streaming
        let request = ChatRequest {
            model: self.config.primary_model.clone(),
            messages: messages.clone(),
            stream: true,
            max_tokens: None,
            temperature: None,
            tools: Some(tools.clone()),
        };

        let (mut rx, mut primary_metrics) = self.primary_provider.chat_stream_realtime(request).await?;
        let mut primary_content = String::new();
        let mut primary_reasoning = String::new();
        let mut primary_tool_calls: Vec<ToolCallRequest> = Vec::new();
        let mut primary_finish_reason: Option<String> = None;
        let mut accumulated_tool_call: Option<AccumulatedToolCall> = None;

        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Reasoning(r) => {
                    primary_reasoning.push_str(&r);
                    let _ = tx.send(HarnessEvent::ReasoningChunk(r));
                }
                StreamChunk::Content(c) => {
                    if primary_metrics.tokens_generated == 0 && !c.is_empty() {
                        // first content chunk marker
                    }
                    primary_content.push_str(&c);
                    let _ = tx.send(HarnessEvent::Chunk(c));
                }
                StreamChunk::ToolCall { id, name, arguments } => {
                    let tool_call_id = id.clone();
                    let tool_call_name = name.clone();
                    let tool_call_args = arguments.clone();
                    if let Some(prev) = accumulated_tool_call.take() {
                        primary_tool_calls.push(ToolCallRequest {
                            id: prev.id,
                            r#type: "function".to_string(),
                            function: crate::providers::ToolCallFunction {
                                name: prev.name,
                                arguments: prev.arguments,
                            },
                        });
                    }
                    accumulated_tool_call = Some(AccumulatedToolCall { id, name, arguments });
                    let _ = tx.send(HarnessEvent::ToolCall {
                        id: tool_call_id,
                        name: tool_call_name,
                        arguments: tool_call_args,
                    });
                }
                StreamChunk::Finish(fr) => {
                    primary_finish_reason = Some(fr.clone());
                    if fr == "tool_calls" {
                        // Tool calls will be emitted when the stream ends normally.
                        break;
                    }
                }
            }
        }

        // Flush any final accumulated tool call
        if let Some(prev) = accumulated_tool_call.take() {
            primary_tool_calls.push(ToolCallRequest {
                id: prev.id,
                r#type: "function".to_string(),
                function: crate::providers::ToolCallFunction {
                    name: prev.name,
                    arguments: prev.arguments,
                },
            });
        }

        // Reconstruct the stream metrics if the provider didn't supply them
        primary_metrics.tokens_generated = primary_content.split_whitespace().count() as u32;

        let primary_response = ModelResponse {
            model_name: self.config.primary_model.clone(),
            provider_name: "primary".to_string(),
            content: primary_content.clone(),
            reasoning: if primary_reasoning.is_empty() { None } else { Some(primary_reasoning.clone()) },
            tool_calls: primary_tool_calls.clone(),
            metrics: primary_metrics.clone(),
            finish_reason: primary_finish_reason.clone(),
        };

        let _ = tx.send(HarnessEvent::AssistantComplete {
            content: primary_content.clone(),
            reasoning: primary_response.reasoning.clone(),
            tool_calls: primary_tool_calls.clone(),
            metrics: primary_metrics.clone(),
            finish_reason: primary_finish_reason.clone(),
        });

        // Handle tool calls if present
        let mut tool_results = Vec::new();
        let mut had_tool_calls = false;
        let mut final_content = primary_content.clone();

        if !primary_tool_calls.is_empty() {
            had_tool_calls = true;

            let mut loop_count = 0;
            let mut current_messages = messages.clone();
            let mut current_tool_calls = primary_tool_calls.clone();

            // Add the assistant message with tool_calls
            current_messages.push(Message {
                role: "assistant".to_string(),
                content: primary_content.clone(),
                images: None,
                tool_call_id: None,
                tool_calls: Some(current_tool_calls.clone()),
                reasoning_content: primary_response.reasoning.clone(),
            });

            while loop_count < self.config.max_tool_loops {
                loop_count += 1;

                // Execute all tool calls in this response
                let mut batch_results = Vec::new();
                for tool_call in &current_tool_calls {
                    let result = self.execute_tool_call(tool_call).await?;
                    let _ = tx.send(HarnessEvent::ToolResult {
                        tool_call_id: result.tool_call_id.clone(),
                        name: result.tool_name.clone(),
                        args: result.args.clone(),
                        result: result.result.clone(),
                        success: result.success,
                    });
                    batch_results.push(result);
                }

                // Add tool results to messages
                for result in &batch_results {
                    current_messages.push(Message {
                        role: "tool".to_string(),
                        content: result.result.clone(),
                        images: None,
                        tool_call_id: Some(result.tool_call_id.clone()),
                        tool_calls: None,
                        reasoning_content: None,
                    });
                }

                tool_results.extend(batch_results);

                // Re-query the model with tool results
                let follow_up_request = ChatRequest {
                    model: self.config.primary_model.clone(),
                    messages: current_messages.clone(),
                    stream: true,
                    max_tokens: None,
                    temperature: None,
                    tools: Some(tools.clone()),
                };

                let (mut follow_rx, _follow_metrics) = self.primary_provider.chat_stream_realtime(follow_up_request).await?;
                let mut follow_content = String::new();
                let mut follow_reasoning = String::new();
                let mut follow_tool_calls: Vec<ToolCallRequest> = Vec::new();
                let mut _follow_finish: Option<String> = None;
                let mut follow_acc: Option<AccumulatedToolCall> = None;

                while let Some(chunk) = follow_rx.recv().await {
                    match chunk {
                        StreamChunk::Reasoning(r) => {
                            follow_reasoning.push_str(&r);
                            let _ = tx.send(HarnessEvent::ReasoningChunk(r));
                        }
                        StreamChunk::Content(c) => {
                            follow_content.push_str(&c);
                            let _ = tx.send(HarnessEvent::Chunk(c));
                        }
                        StreamChunk::ToolCall { id, name, arguments } => {
                            if let Some(prev) = follow_acc.take() {
                                follow_tool_calls.push(ToolCallRequest {
                                    id: prev.id,
                                    r#type: "function".to_string(),
                                    function: crate::providers::ToolCallFunction {
                                        name: prev.name,
                                        arguments: prev.arguments,
                                    },
                                });
                            }
                            follow_acc = Some(AccumulatedToolCall { id, name, arguments });
                        }
                        StreamChunk::Finish(fr) => {
                            _follow_finish = Some(fr.clone());
                            if fr == "tool_calls" {
                                break;
                            }
                        }
                    }
                }

                if let Some(prev) = follow_acc.take() {
                    follow_tool_calls.push(ToolCallRequest {
                        id: prev.id,
                        r#type: "function".to_string(),
                        function: crate::providers::ToolCallFunction {
                            name: prev.name,
                            arguments: prev.arguments,
                        },
                    });
                }

                final_content = follow_content.clone();
                let final_reasoning = if follow_reasoning.is_empty() { None } else { Some(follow_reasoning.clone()) };

                current_messages.push(Message {
                    role: "assistant".to_string(),
                    content: follow_content.clone(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: if follow_tool_calls.is_empty() { None } else { Some(follow_tool_calls.clone()) },
                    reasoning_content: final_reasoning.clone(),
                });

                if follow_tool_calls.is_empty() {
                    break;
                }

                current_tool_calls = follow_tool_calls;
            }

            messages = current_messages;
        } else {
            messages.push(Message {
                role: "assistant".to_string(),
                content: primary_content.clone(),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: primary_response.reasoning.clone(),
            });
        }

        let _ = tx.send(HarnessEvent::FollowUp(final_content.clone()));

        // Save to memory
        if persist {
            self.save_turn_to_memory(user_message, &messages, &tool_results)?;
        }

        // Update state
        self.state.messages = messages;
        self.state.tool_results_history.extend(tool_results.clone());

        // Query secondary models if enabled
        let secondary = if self.config.multi_model_enabled {
            let _ = tx.send(HarnessEvent::SystemMessage("Querying secondary models...".to_string()));
            self.query_secondary_models_streaming(user_message, &tx).await?
        } else {
            Vec::new()
        };

        // Calculate totals
        let total_tokens = primary_metrics.tokens_generated as u64;
        let total_cost = 0.0;

        self.state.total_tokens += total_tokens;
        self.state.total_cost_usd += total_cost;

        let _ = tx.send(HarnessEvent::Done);

        Ok(HarnessResponse {
            primary: primary_response,
            secondary,
            tool_results,
            had_tool_calls,
            total_tokens,
            total_cost_usd: total_cost,
        })
    }

    /// Build the conversation messages for this turn, including system prompt,
    /// memory context, and skills.
    fn build_conversation_messages(
        &self,
        user_message: &str,
    ) -> Result<Vec<Message>> {
        let mut messages = Vec::new();

        // 1. System prompt with soul + skills
        let system_prompt = self.build_system_prompt(user_message)?;
        messages.push(Message {
            role: "system".to_string(),
            content: system_prompt,
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        // 2. Memory context injection
        let relevant_messages = self.get_relevant_memory(user_message)?;
        for msg in relevant_messages {
            messages.push(Message {
                role: msg.role,
                content: msg.content,
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
        }

        // 3. Current conversation history from state
        for msg in &self.state.messages {
            messages.push(msg.clone());
        }

        // 4. Current user message
        messages.push(Message {
            role: "user".to_string(),
            content: user_message.to_string(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        Ok(messages)
    }

    /// Build the system prompt including agent soul and triggered skills.
    fn build_system_prompt(
        &self,
        user_message: &str,
    ) -> Result<String> {
        let soul = crate::agent::soul::load_soul_from_config(&self.app_config);
        let mut prompt = soul.system_prompt();

        // Add tool instructions
        prompt.push_str("\n\n## AVAILABLE TOOLS\n");
        prompt.push_str("You have access to tools. When you need to use a tool, respond with a tool call. ");
        prompt.push_str("The system will execute the tool and return the result to you. ");
        prompt.push_str("You can then use that result to formulate your final response.\n");

        // Add triggered skills
        if self.config.skills_enabled {
            if let Some(ref registry) = self.skill_registry {
                let triggered = registry.find_triggered(user_message);
                if !triggered.is_empty() {
                    let skills_prompt = format_skills_prompt(&triggered);
                    prompt.push_str(&skills_prompt);
                }
            }
        }

        // Add memory context hint
        prompt.push_str("\n\n## MEMORY\n");
        prompt.push_str("You have access to persistent memory across sessions. ");
        prompt.push_str("Previous relevant context has been injected into this conversation.\n");

        Ok(prompt)
    }

    /// Get relevant past messages from memory for context injection.
    fn get_relevant_memory(
        &self,
        query: &str,
    ) -> Result<Vec<MemoryMessage>> {
        let limit = self.config.memory_context_limit;

        // Try semantic search first
        let semantic_results = self.memory.semantic_search(query, limit)?;
        if !semantic_results.is_empty() {
            return Ok(semantic_results.into_iter().map(|(msg, _score)| msg).collect());
        }

        // Fall back to keyword search
        let keyword_results = self.memory.search_messages(query, limit)?;
        Ok(keyword_results)
    }

    /// Query the primary model and parse the response.
    async fn query_primary(
        &self,
        request: ChatRequest,
    ) -> Result<ModelResponse> {
        let start = Instant::now();

        let response = self.primary_provider.chat(request).await?;
        let choice = response.choices.first().context("No response from primary model")?;

        let metrics = StreamMetrics {
            first_token_latency_ms: start.elapsed().as_millis() as u64,
            total_latency_ms: start.elapsed().as_millis() as u64,
            tokens_generated: response.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
            cached: false,
        };

        // Extract reasoning content if present
        let reasoning = choice.message.reasoning_content.clone();

        // Extract tool calls
        let tool_calls = choice.message.tool_calls.clone().unwrap_or_default();

        Ok(ModelResponse {
            model_name: self.config.primary_model.clone(),
            provider_name: "primary".to_string(),
            content: choice.message.content.clone(),
            reasoning,
            tool_calls,
            metrics,
            finish_reason: choice.finish_reason.clone(),
        })
    }

    /// Query secondary models in parallel for multi-model comparison.
    async fn query_secondary_models(
        &self,
        user_message: &str,
    ) -> Result<Vec<ModelResponse>> {
        let mut tasks = Vec::new();

        for (model_name, provider) in &self.secondary_providers {
            let msg = user_message.to_string();
            let model = model_name.clone();
            let prov = provider.clone();

            let task = tokio::spawn(async move {
                let request = ChatRequest::new(
                    model.clone(),
                    vec![Message::text("user", msg)],
                    false,
                );

                let start = Instant::now();
                match prov.chat(request).await {
                    Ok(response) => {
                        let choice = response.choices.first()?;
                        Some(ModelResponse {
                            model_name: model,
                            provider_name: "secondary".to_string(),
                            content: choice.message.content.clone(),
                            reasoning: choice.message.reasoning_content.clone(),
                            tool_calls: choice.message.tool_calls.clone().unwrap_or_default(),
                            metrics: StreamMetrics {
                                first_token_latency_ms: start.elapsed().as_millis() as u64,
                                total_latency_ms: start.elapsed().as_millis() as u64,
                                tokens_generated: response.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
                                cached: false,
                            },
                            finish_reason: choice.finish_reason.clone(),
                        })
                    }
                    Err(_) => None,
                }
            });

            tasks.push(task);
        }

        let mut responses = Vec::new();
        for task in tasks {
            if let Ok(Some(response)) = task.await {
                responses.push(response);
            }
        }

        Ok(responses)
    }

    /// Query secondary models in parallel and emit streaming events.
    async fn query_secondary_models_streaming(
        &self,
        user_message: &str,
        tx: &mpsc::UnboundedSender<HarnessEvent>,
    ) -> Result<Vec<ModelResponse>> {
        let mut responses = Vec::new();

        for (model_name, provider) in &self.secondary_providers {
            let model = model_name.clone();
            let prov = provider.clone();
            let msg = user_message.to_string();
            let tx = tx.clone();

            let task = tokio::spawn(async move {
                let request = ChatRequest::new(
                    model.clone(),
                    vec![Message::text("user", msg)],
                    false,
                );

                let start = Instant::now();
                match prov.chat(request).await {
                    Ok(response) => {
                        let choice = response.choices.first()?;
                        let metrics = StreamMetrics {
                            first_token_latency_ms: start.elapsed().as_millis() as u64,
                            total_latency_ms: start.elapsed().as_millis() as u64,
                            tokens_generated: response.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
                            cached: false,
                        };
                        let resp = ModelResponse {
                            model_name: model.clone(),
                            provider_name: "secondary".to_string(),
                            content: choice.message.content.clone(),
                            reasoning: choice.message.reasoning_content.clone(),
                            tool_calls: choice.message.tool_calls.clone().unwrap_or_default(),
                            metrics: metrics.clone(),
                            finish_reason: choice.finish_reason.clone(),
                        };
                        let _ = tx.send(HarnessEvent::MultiModelResponse {
                            name: model,
                            content: choice.message.content.clone(),
                            metrics,
                        });
                        Some(resp)
                    }
                    Err(_) => None,
                }
            });

            if let Ok(Some(response)) = task.await {
                responses.push(response);
            }
        }

        Ok(responses)
    }

    /// Execute a single tool call with security gating.
    async fn execute_tool_call(
        &self,
        tool_call: &ToolCallRequest,
    ) -> Result<ToolExecutionResult> {
        let start = Instant::now();
        let tool_name = &tool_call.function.name;
        let args = &tool_call.function.arguments;

        // Security check
        match self.security_engine.check_tool_call(tool_name, args) {
            SecurityDecision::Allow => {}
            SecurityDecision::RequireApproval { reason, risk_level } => {
                if self.config.require_tool_approval {
                    return Ok(ToolExecutionResult {
                        tool_call_id: tool_call.id.clone(),
                        tool_name: tool_name.clone(),
                        args: args.clone(),
                        result: format!("Approval required: {} (risk: {:?})", reason, risk_level),
                        success: false,
                        execution_time_ms: start.elapsed().as_millis() as u64,
                    });
                }
            }
            SecurityDecision::Deny { reason } => {
                self.security_engine.audit(
                    tool_name,
                    args,
                    false,
                    crate::security::RiskLevel::Critical,
                    &reason,
                );
                return Ok(ToolExecutionResult {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_name.clone(),
                    args: args.clone(),
                    result: format!("Security denied: {}", reason),
                    success: false,
                    execution_time_ms: start.elapsed().as_millis() as u64,
                });
            }
        }

        // Normalize args and execute
        let normalized_args = normalize_tool_args(tool_name, args);
        let result = match execute_tool(tool_name, &normalized_args) {
            Some(Ok(output)) => {
                let sanitized = self.security_engine.sanitize_output(tool_name, &output);
                self.security_engine.audit(
                    tool_name,
                    args,
                    true,
                    crate::security::RiskLevel::Low,
                    "approved",
                );
                ToolExecutionResult {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_name.clone(),
                    args: args.clone(),
                    result: sanitized,
                    success: true,
                    execution_time_ms: start.elapsed().as_millis() as u64,
                }
            }
            Some(Err(e)) => {
                self.security_engine.audit(
                    tool_name,
                    args,
                    false,
                    crate::security::RiskLevel::High,
                    &e.to_string(),
                );
                ToolExecutionResult {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_name.clone(),
                    args: args.clone(),
                    result: e.to_string(),
                    success: false,
                    execution_time_ms: start.elapsed().as_millis() as u64,
                }
            }
            None => ToolExecutionResult {
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_name.clone(),
                args: args.clone(),
                result: format!("Unknown tool: {}", tool_name),
                success: false,
                execution_time_ms: start.elapsed().as_millis() as u64,
            },
        };

        Ok(result)
    }

    /// Save the current turn to memory.
    /// The `messages` slice already includes the user message and all assistant/tool messages.
    fn save_turn_to_memory(
        &self,
        _user_message: &str,
        messages: &[Message],
        tool_results: &[ToolExecutionResult],
    ) -> Result<()> {
        // Save all messages from this turn (user + assistant + tool).
        for msg in messages {
            if msg.role == "user" || msg.role == "assistant" || msg.role == "tool" {
                let memory_msg = MemoryMessage {
                    id: Uuid::new_v4().to_string(),
                    session_id: self.session_id.clone(),
                    role: msg.role.clone(),
                    content: msg.content.clone(),
                    created_at: Utc::now(),
                    tokens_used: None,
                };
                self.memory.save_message(&memory_msg)?;
            }
        }

        // Save tool calls
        for result in tool_results {
            let tool_call = MemoryToolCall {
                id: result.tool_call_id.clone(),
                session_id: self.session_id.clone(),
                tool_name: result.tool_name.clone(),
                args: result.args.clone(),
                result: result.result.clone(),
                success: result.success,
                created_at: Utc::now(),
            };
            self.memory.save_tool_call(&tool_call)?;
        }

        Ok(())
    }

    /// Get the current harness state.
    pub fn state(&self) -> &HarnessState {
        &self.state
    }

    /// Get mutable access to the harness state.
    pub fn state_mut(&mut self) -> &mut HarnessState {
        &mut self.state
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_harness_config_default() {
        let config = HarnessConfig::default();
        assert_eq!(config.max_tool_loops, 10);
        assert!(!config.require_tool_approval);
        assert!(!config.multi_model_enabled);
        assert_eq!(config.memory_context_limit, 5);
        assert!(config.skills_enabled);
    }

    #[test]
    fn test_harness_state_new() {
        let state = HarnessState::new("test-session".to_string());
        assert_eq!(state.session_id, "test-session");
        assert!(state.messages.is_empty());
        assert_eq!(state.turn_count, 0);
        assert_eq!(state.total_tokens, 0);
    }

    #[test]
    fn test_model_response_creation() {
        let response = ModelResponse {
            model_name: "test-model".to_string(),
            provider_name: "test".to_string(),
            content: "Hello".to_string(),
            reasoning: None,
            tool_calls: Vec::new(),
            metrics: StreamMetrics {
                first_token_latency_ms: 100,
                total_latency_ms: 500,
                tokens_generated: 10,
                cached: false,
            },
            finish_reason: Some("stop".to_string()),
        };
        assert_eq!(response.content, "Hello");
        assert!(response.tool_calls.is_empty());
    }

    #[test]
    fn test_tool_execution_result() {
        let result = ToolExecutionResult {
            tool_call_id: "call_123".to_string(),
            tool_name: "terminal".to_string(),
            args: "ls".to_string(),
            result: "file1.txt\nfile2.txt".to_string(),
            success: true,
            execution_time_ms: 50,
        };
        assert!(result.success);
        assert_eq!(result.tool_name, "terminal");
    }

    #[test]
    fn test_harness_response_creation() {
        let response = HarnessResponse {
            primary: ModelResponse {
                model_name: "primary".to_string(),
                provider_name: "test".to_string(),
                content: "Done".to_string(),
                reasoning: None,
                tool_calls: Vec::new(),
                metrics: StreamMetrics {
                    first_token_latency_ms: 0,
                    total_latency_ms: 0,
                    tokens_generated: 0,
                    cached: false,
                },
                finish_reason: None,
            },
            secondary: Vec::new(),
            tool_results: Vec::new(),
            had_tool_calls: false,
            total_tokens: 0,
            total_cost_usd: 0.0,
        };
        assert!(!response.had_tool_calls);
        assert_eq!(response.primary.content, "Done");
    }
}
