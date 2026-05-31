use anyhow::Result;
use arboard::Clipboard;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame, Terminal,
};
use std::time::{Duration, Instant};

use crate::agent::{Agent, AgentConfig};
use crate::config::Config;
use crate::memory::{ContextInjector, MemoryStore, Message as MemoryMessage, ToolCall};
use crate::providers::{ChatRequest, Message, Provider, StreamMetrics};
use crate::tools::{detect_tool_suggestions, find_tool, get_tools, AsyncToolExecutor, ToolSuggestion};
use chrono::Utc;
use unicode_width::UnicodeWidthChar;
use uuid::Uuid;
use crate::skills::SkillRegistry;

mod theme;
use theme::*;

mod ascii_art;

#[allow(dead_code)]
const MAX_CONTEXT_MESSAGES: usize = 5;
const TICK_RATE: Duration = Duration::from_millis(16); // ~60fps for responsive input

/// Events sent from the background streaming task to the main TUI loop.
#[derive(Debug)]
enum StreamEvent {
    /// Streaming started — show the user that we're waiting.
    Start,
    /// A content chunk arrived (for true streaming UIs).
    Chunk(String),
    /// The full response is complete.
    ResponseComplete { content: String, metrics: StreamMetrics },
    /// A tool was invoked and returned a result.
    ToolResult {
        name: String,
        args: String,
        result: String,
        success: bool,
    },
    /// Follow-up assistant message after tool execution.
    FollowUp(String),
    /// Multi-model secondary response.
    MultiModelResponse { name: String, content: String, metrics: StreamMetrics },
    /// Switch to tool-approval mode.
    #[allow(dead_code)]
    SetPendingSuggestion(ToolSuggestion),
    /// An error occurred.
    Error(String),
    /// A system/info message (e.g. auto-execution notice).
    SystemMessage(String),
    /// Streaming finished (success or error).
    Done,
}

/// A secondary model response attached to a primary assistant message.
#[derive(Debug, Clone)]
struct SecondaryResponse {
    model_name: String,
    content: String,
    latency_ms: u64,
    tokens: u32,
}

/// A single message in the chat history.
#[derive(Debug, Clone)]
struct ChatMessage {
    role: String,
    content: String,
    /// Optional image attachments as base64 data URLs.
    images: Option<Vec<String>>,
    #[allow(dead_code)]
    timestamp: chrono::DateTime<Utc>,
    /// Secondary responses from other models (multi-model mode).
    multi_model_responses: Vec<SecondaryResponse>,
}

/// Application state for the TUI.
struct App {
    /// User input buffer.
    input: String,
    /// Cursor position in input.
    cursor_position: usize,
    /// Chat history (scrollable).
    messages: Vec<ChatMessage>,
    /// Scroll offset for chat history.
    scroll: usize,
    /// Whether the app should exit.
    should_exit: bool,
    /// Ctrl+C press counter for double-tap quit.
    ctrl_c_count: u8,
    /// Last Ctrl+C timestamp for debounce.
    last_ctrl_c: Option<Instant>,
    /// Current mode: normal, agent, or tool_approval.
    mode: AppMode,
    /// Session ID.
    session_id: String,
    /// Current model.
    model: String,
    /// Current model's context length.
    model_context_length: usize,
    /// Current model config (for native params).
    model_config: Option<crate::config::ModelConfig>,
    /// Whether we're currently streaming a response.
    is_streaming: bool,
    /// Partial content during streaming.
    streaming_content: String,
    /// Tool suggestion pending approval.
    pending_suggestion: Option<ToolSuggestion>,
    /// Receiver for background stream events.
    stream_rx: Option<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>>,
    /// Memory store for persistence.
    memory: MemoryStore,
    /// Provider for API calls.
    provider: Provider,
    /// Message history for the model.
    model_messages: Vec<Message>,
    /// Start time for session.
    session_start: Instant,
    /// Token usage tracking (estimated).
    tokens_used: u64,
    /// Tool calls count.
    tool_calls_count: usize,
    /// Config reference.
    config: Config,
    /// Project path.
    #[allow(dead_code)]
    project_path: String,
    /// Sidebar expanded.
    sidebar_expanded: bool,
    focused_pane: usize,
    branches: Vec<SessionBranch>,
    active_branch: usize,
    multi_model_mode: bool,
    /// Secondary providers for multi-model mode.
    secondary_providers: Vec<(String, Provider)>,
    /// Show the multi-model comparison overlay.
    show_comparison: bool,
    /// Selected response index in the comparison overlay.
    comparison_selected: usize,
    /// Security engine for guardrails.
    security_engine: crate::security::SecurityEngine,
    /// Autonomous mode — temporarily elevate risk tolerance for full-send coding.
    autonomous_mode: bool,
    /// MCP manager for external tool servers.
    mcp_manager: Option<std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpManager>>>,
    /// Timestamp when tool approval popup was shown (for auto-close timeout).
    tool_approval_shown_at: Option<Instant>,
    /// Evolution engine for self-adaptive behavior.
    evolution: Option<crate::evolution::EvolutionEngine>,
    /// Sidebar tab: 0=Tools, 1=Skills.
    sidebar_tab: usize,
    /// Scroll offset for sidebar tool/skill list.
    sidebar_scroll: usize,
    /// Per-session performance metrics.
    session_perf: SessionPerformance,
    /// Skill registry for loaded skills.
    skill_registry: Option<crate::skills::SkillRegistry>,
    /// Swarm engine for multi-agent mode.
    swarm: Option<crate::swarm::SwarmEngine>,
    /// Whether swarm mode is active in the sidebar.
    swarm_active: bool,
    /// Cached swarm agent snapshot for sync rendering.
    swarm_agents: Vec<crate::swarm::SwarmAgent>,
    /// Cached swarm running state.
    swarm_running: bool,
    /// Pending image attachment for the next user message.
    pending_image: Option<String>,
    /// Context compression engine.
    compressor: Option<crate::memory::compression::ContextCompressor>,
}

#[derive(Debug, Clone)]
struct SessionBranch {
    name: String,
    messages: Vec<ChatMessage>,
    model_messages: Vec<Message>,
    #[allow(dead_code)]
    created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
enum AppMode {
    Normal,
    Agent,
    ToolApproval,
}

#[derive(Debug, Clone, Default)]
struct SessionPerformance {
    first_token_ms: Vec<u64>,
    total_latency_ms: Vec<u64>,
    tool_exec_ms: Vec<u64>,
    requests: usize,
    tools: usize,
}

impl SessionPerformance {
    fn record_response(&mut self, metrics: &StreamMetrics) {
        self.first_token_ms.push(metrics.first_token_latency_ms);
        self.total_latency_ms.push(metrics.total_latency_ms);
        self.requests += 1;
    }

    fn record_tool_exec(&mut self, duration_ms: u64) {
        self.tool_exec_ms.push(duration_ms);
        self.tools += 1;
    }

    fn avg_first_token(&self) -> u64 {
        if self.first_token_ms.is_empty() { 0 } else {
            self.first_token_ms.iter().sum::<u64>() / self.first_token_ms.len() as u64
        }
    }

    fn avg_total_latency(&self) -> u64 {
        if self.total_latency_ms.is_empty() { 0 } else {
            self.total_latency_ms.iter().sum::<u64>() / self.total_latency_ms.len() as u64
        }
    }

    fn avg_tool_exec(&self) -> u64 {
        if self.tool_exec_ms.is_empty() { 0 } else {
            self.tool_exec_ms.iter().sum::<u64>() / self.tool_exec_ms.len() as u64
        }
    }
}

impl App {
    fn new(config: Config) -> Result<Self> {
        let (provider_name, provider_config) = config.find_provider_for_model(&config.default_model)
            .unwrap_or_else(|| config.providers.iter().next()
                .map(|(name, cfg)| (name.clone(), cfg.clone()))
                .unwrap_or_else(|| ("local".to_string(), crate::config::ProviderConfig {
                    base_url: "http://127.0.0.1:8080/v1".to_string(),
                    api_key: "local".to_string(),
                    models: vec![],
                    kind: crate::config::ProviderKind::OpenAiCompatible,
                    headers: std::collections::HashMap::new(),
                    env_file: None,
                })));

        let provider = Provider::new(
            provider_name.clone(),
            provider_config.base_url.clone(),
            provider_config.api_key.clone(),
            provider_config.kind.clone(),
            provider_config.headers.clone(),
        );

        let memory = MemoryStore::new(&config.memory_db_path)?;
        let session_id = Uuid::new_v4().to_string();
        let model = config.default_model.clone();

        let model_config = provider_config.models.iter()
            .find(|m| m.name == model)
            .cloned();
        let model_context_length = model_config.as_ref()
            .map(|m| m.context_length)
            .unwrap_or(128000);

        let project_path = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        if project_path.is_empty() {
            memory.create_session(&session_id, &model, "general")?;
        } else {
            memory.create_session_with_project(&session_id, &model, "general", &project_path)?;
        }

        let soul = crate::agent::soul::load_soul_from_config(&config);

        // Build filesystem capabilities description
        let fs_capabilities = if config.filesystem.allowed_paths.is_empty() {
            "You have FULL filesystem access to the entire system. \
             You can read, write, list, and search any directory.".to_string()
        } else {
            let paths = config.filesystem.allowed_paths.join(", ");
            format!(
                "You have filesystem access to the following directories: {}. \
                 You can read files, list directories, search for files, and inspect configs. \
                 Use the fs tool to explore: fs read <path>, fs list <path>, \
                 fs tree <path>, fs find <path> <name>, fs glob <pattern>, \
                 fs stat <path>, fs cat <path> [offset] [limit].",
                paths
            )
        };

        let system_msg = Message {
            role: "system".to_string(),
            content: format!(
                "{}\n\n{}\n\nYou have access to tools. \
                 When you need to use a tool, output it as: TOOL:<tool_name> <args> \
                 Low and Medium risk tools execute automatically. \
                 High risk tools (curl, ssh, redirects, sudo) require user approval. \
                 You can analyze images when users attach them. \
                 Be concise and direct. Don't overthink.",
                soul.system_prompt(),
                fs_capabilities
            ),
            images: None,
        };

        let security_engine = crate::security::SecurityEngine::new(
            crate::security::SecurityConfig::load().unwrap_or_default()
        )?;

        Ok(Self {
            input: String::new(),
            cursor_position: 0,
            messages: Vec::new(),
            scroll: 0,
            should_exit: false,
            mode: AppMode::Normal,
            session_id: session_id.clone(),
            model: model.clone(),
            model_context_length,
            model_config,
            is_streaming: false,
            streaming_content: String::new(),
            pending_suggestion: None,
            stream_rx: None,
            memory,
            provider,
            model_messages: vec![system_msg.clone()],
            session_start: Instant::now(),
            tokens_used: 0,
            tool_calls_count: 0,
            config: config.clone(),
            project_path,
            sidebar_expanded: true,
            focused_pane: 1,
            ctrl_c_count: 0,
            last_ctrl_c: None,
            branches: vec![SessionBranch {
                name: "main".to_string(),
                messages: Vec::new(),
                model_messages: vec![system_msg.clone()],
                created_at: Utc::now(),
            }],
            active_branch: 0,
            multi_model_mode: false,
            secondary_providers: Vec::new(),
            show_comparison: false,
            comparison_selected: 0,
            security_engine,
            autonomous_mode: false,
            mcp_manager: None,
            tool_approval_shown_at: None,
            evolution: crate::evolution::EvolutionEngine::new(&config).ok(),
            sidebar_tab: 0,
            sidebar_scroll: 0,
            session_perf: SessionPerformance::default(),
            skill_registry: {
                let skills_dir = dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("openshark")
                    .join("skills");
                SkillRegistry::new(skills_dir).ok()
            },
            swarm: None,
            swarm_active: false,
            swarm_agents: Vec::new(),
            swarm_running: false,
            pending_image: None,
            compressor: Some(crate::memory::compression::ContextCompressor::new(
                config.context_compression.clone(),
            )),
        })
    }

    fn create_branch(&mut self, name: &str) {
        let branch = SessionBranch {
            name: name.to_string(),
            messages: self.messages.clone(),
            model_messages: self.model_messages.clone(),
            created_at: Utc::now(),
        };
        self.branches.push(branch);
        self.active_branch = self.branches.len() - 1;
        self.add_system_message(format!(
            "Created branch '{}' ({} total branches)",
            name, self.branches.len()
        ));
    }

    fn switch_branch(&mut self, index: usize) -> Result<()> {
        if index >= self.branches.len() {
            return Err(anyhow::anyhow!(
                "Branch {} does not exist. Use /branches to list.",
                index
            ));
        }
        self.save_current_branch();
        let branch = &self.branches[index];
        self.messages = branch.messages.clone();
        self.model_messages = branch.model_messages.clone();
        self.active_branch = index;
        self.add_system_message(format!(
            "Switched to branch '{}' ({})",
            branch.name, index
        ));
        Ok(())
    }

    fn save_current_branch(&mut self) {
        if let Some(branch) = self.branches.get_mut(self.active_branch) {
            branch.messages = self.messages.clone();
            branch.model_messages = self.model_messages.clone();
        }
    }

    /// Initialize MCP connections and register discovered tools.
    async fn init_mcp(&mut self) {
        let manager = crate::mcp::McpManager::new();
        let arc_manager = std::sync::Arc::new(tokio::sync::Mutex::new(manager));

        {
            let mgr = arc_manager.lock().await;
            if let Err(e) = mgr.connect_all(&self.config.gateway.mcp.servers).await {
                tracing::warn!("MCP connect_all error: {}", e);
            }

            // Discover and register MCP tools into the global tool cache
            let mcp_tools = mgr.all_tools().await;
            let mut adapted_tools: Vec<std::sync::Arc<dyn crate::tools::Tool>> = Vec::new();
            for (server_name, tool) in mcp_tools {
                adapted_tools.push(std::sync::Arc::new(
                    crate::tools::mcp::McpToolAdapter::new(
                        tool,
                        server_name,
                        std::sync::Arc::clone(&arc_manager),
                    )
                ));
            }
            if !adapted_tools.is_empty() {
                crate::tools::register_mcp_tools(adapted_tools);
                self.add_system_message(format!(
                    "🔧 Registered {} MCP tools globally",
                    crate::tools::get_tools().len() - 9 // 9 native tools
                ));
            }

            let status = mgr.status().await;
            for (name, connected, tool_count) in status {
                let status_str = if connected { "✅" } else { "❌" };
                self.add_system_message(format!(
                    "🔌 MCP server {} {} — {} tools discovered",
                    name, status_str, tool_count
                ));
            }
        }

        self.mcp_manager = Some(arc_manager);
    }

    /// Shutdown MCP connections.
    async fn shutdown_mcp(&mut self) {
        if let Some(manager) = self.mcp_manager.take() {
            let mgr = manager.lock().await;
            if let Err(e) = mgr.disconnect_all().await {
                tracing::warn!("MCP disconnect error: {}", e);
            }
        }
    }

    fn list_branches(&mut self) {
        let mut msg = format!("Branches ({} total):\n", self.branches.len());
        for (i, branch) in self.branches.iter().enumerate() {
            let marker = if i == self.active_branch { "●" } else { "○" };
            msg.push_str(&format!(
                "  {} {}: {} messages\n",
                marker,
                branch.name,
                branch.messages.len()
            ));
        }
        msg.push_str("\nUse /branch <name> to create, /switch <index> to change");
        self.add_system_message(msg);
    }

    fn toggle_multi_model(&mut self) {
        self.multi_model_mode = !self.multi_model_mode;
        if self.multi_model_mode {
            self.secondary_providers = self.config.providers.iter()
                .filter(|(name, _)| **name != "kimi")
                .map(|(name, provider)| {
                    (name.clone(), Provider::new(
                        name.clone(),
                        provider.base_url.clone(),
                        provider.api_key.clone(),
                        provider.kind.clone(),
                        provider.headers.clone(),
                    ))
                })
                .collect();
            self.add_system_message(
                "Multi-model mode ON. Responses will stream from all models.".to_string()
            );
        } else {
            self.secondary_providers.clear();
            self.add_system_message("Multi-model mode OFF.".to_string());
        }
    }

    fn show_model_selector(&mut self) {
        let mut msg = String::from("Available models:\n");
        let mut all_models: Vec<(String, String, usize)> = Vec::new(); // (display, provider_name, ctx_len)

        // 1. Static models from config
        for (provider_name, provider) in &self.config.providers {
            for m in &provider.models {
                all_models.push((
                    format!("{} ({})", m.name, provider_name),
                    provider_name.clone(),
                    m.context_length,
                ));
            }
        }

        // 2. Dynamic models from local provider's /v1/models endpoint
        // Skip dynamic model fetching in the TUI — it requires async and we're in a sync context.
        // The static models from config are sufficient for the selector.
        // Dynamic models can be refreshed via the CLI `openshark models` command.

        for (i, (display, _provider_name, _ctx_len)) in all_models.iter().enumerate() {
            let indicator = if self.model == display.split(" (").next().unwrap_or("") {
                "●"
            } else {
                "○"
            };
            msg.push_str(&format!("  {} {} (type /model {} to switch)\n", indicator, display, i)
            );
        }
        msg.push_str("\nOr type: /model <model_name>");
        self.add_system_message(msg);
    }

    fn switch_model(&mut self, model_name: &str) -> Result<()> {
        for (provider_name, provider) in &self.config.providers {
            if let Some(model_config) = provider.models.iter().find(|m| m.name == model_name) {
                self.model = model_config.name.clone();
                self.model_context_length = model_config.context_length;
                self.model_config = Some(model_config.clone());
                self.provider = Provider::new(
                    provider_name.clone(),
                    provider.base_url.clone(),
                    provider.api_key.clone(),
                    provider.kind.clone(),
                    provider.headers.clone(),
                );
                self.add_system_message(format!(
                    "Switched to model: {} (provider: {}, ctx={})",
                    model_config.name, provider_name, model_config.context_length
                ));
                return Ok(());
            }
        }
        Err(anyhow::anyhow!("Model '{}' not found in config. Run /models to see available models.", model_name))
    }

    fn add_user_message(&mut self, content: String) {
        let token_count = content.split_whitespace().count() as u64;
        let images = self.pending_image.take();
        let has_image = images.is_some();
        let msg = ChatMessage {
            role: "user".to_string(),
            content: content.clone(),
            images: images.as_ref().map(|img| vec![img.clone()]),
            timestamp: Utc::now(),
            multi_model_responses: Vec::new(),
        };
        self.messages.push(msg);

        let memory_msg = MemoryMessage {
            id: Uuid::new_v4().to_string(),
            session_id: self.session_id.clone(),
            role: "user".to_string(),
            content: content.clone(),
            created_at: Utc::now(),
            tokens_used: None,
        };
        let _ = self.memory.save_message(&memory_msg);

        if has_image {
            self.model_messages.push(Message::with_image(
                "user",
                content,
                images.unwrap(),
            ));
        } else {
            self.model_messages.push(Message {
                role: "user".to_string(),
                content,
                images: None,
            });
        }

        self.tokens_used += token_count;
    }

    fn add_assistant_message(&mut self, content: String) {
        let token_count = content.split_whitespace().count() as u64;
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: content.clone(),
            images: None,
            timestamp: Utc::now(),
            multi_model_responses: Vec::new(),
        };
        self.messages.push(msg);

        let memory_msg = MemoryMessage {
            id: Uuid::new_v4().to_string(),
            session_id: self.session_id.clone(),
            role: "assistant".to_string(),
            content: content.clone(),
            created_at: Utc::now(),
            tokens_used: None,
        };
        let _ = self.memory.save_message(&memory_msg);

        self.model_messages.push(Message {
            role: "assistant".to_string(),
            content,
            images: None,
        });

        self.tokens_used += token_count;
    }

    /// Add a system/tool message to the chat.
    fn add_system_message(&mut self, content: String) {
        let msg = ChatMessage {
            role: "system".to_string(),
            content: content.clone(),
            images: None,
            timestamp: Utc::now(),
            multi_model_responses: Vec::new(),
        };
        self.messages.push(msg);
    }

    /// Apply a stream event from the background task.
    fn apply_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Start => {
                self.is_streaming = true;
                self.streaming_content.clear();
            }
            StreamEvent::Chunk(chunk) => {
                self.streaming_content.push_str(&chunk);
            }
            StreamEvent::ResponseComplete { content, metrics } => {
                self.is_streaming = false;
                self.session_perf.record_response(&metrics);
                let _ = self.memory.save_performance_metric(
                    "first_token",
                    &self.model,
                    metrics.first_token_latency_ms,
                    Some(&format!("cached={}", metrics.cached)),
                );
                let _ = self.memory.save_performance_metric(
                    "total_latency",
                    &self.model,
                    metrics.total_latency_ms,
                    Some(&format!("tokens={}", metrics.tokens_generated)),
                );

                // Check for embedded TOOL: lines anywhere in the response
                let embedded_tools = parse_embedded_tools(&content);
                if !embedded_tools.is_empty() {
                    // Display the assistant's message with tool lines stripped
                    let display_content = strip_tool_lines(&content);
                    if !display_content.trim().is_empty() {
                        self.add_assistant_message(display_content);
                    }
                    // Store tool invocations in model messages for follow-up context
                    for (tool_name, args) in &embedded_tools {
                        self.model_messages.push(Message {
                            role: "assistant".to_string(),
                            content: format!("TOOL:{} {}", tool_name, args),
                            images: None,
                        });
                    }
                } else if content.starts_with("TOOL:") {
                    let rest = &content[5..];
                    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                    if !parts.is_empty() {
                        let tool_name = parts[0].trim().to_string();
                        let args = parts.get(1).unwrap_or(&"").trim().to_string();
                        self.add_assistant_message(format!("🔧 Using tool: {} {}", tool_name, args));
                        // Store tool invocation in model messages for follow-up
                        self.model_messages.push(Message {
                            role: "assistant".to_string(),
                            content: format!("TOOL:{} {}", tool_name, args),
                            images: None,
                        });
                    }
                } else {
                    self.add_assistant_message(content.clone());
                    // Tool suggestions are now handled in the background task (stream_model_response_task)
                    // to ensure proper execution → result → follow-up flow.
                    // The UI only handles approval-required tools here.
                    if let Some(suggestion) = detect_high_confidence_suggestion(&content) {
                        match self.security_engine.check_tool_call(
                            &suggestion.tool_name,
                            &suggestion.args
                        ) {
                            crate::security::SecurityDecision::RequireApproval { reason: _, risk_level } => {
                                let tool_name = suggestion.tool_name.clone();
                                self.pending_suggestion = Some(suggestion);
                                self.mode = AppMode::ToolApproval;
                                self.tool_approval_shown_at = Some(Instant::now());
                                self.add_system_message(format!(
                                    "🔒 Tool '{}' requires approval (risk: {:?}) — press y/n",
                                    tool_name, risk_level
                                ));
                            }
                            crate::security::SecurityDecision::Deny { reason } => {
                                self.add_system_message(format!(
                                    "🚫 Tool '{}' blocked: {}",
                                    suggestion.tool_name, reason
                                ));
                            }
                            // Allow case is handled in background task — do nothing here
                            crate::security::SecurityDecision::Allow => {}
                        }
                    }
                }
            }
            StreamEvent::ToolResult { name, args, result, success } => {
                let display = if success {
                    format!("Result: {}", &result[..result.len().min(200)])
                } else {
                    format!("Tool execution failed: {}", result)
                };
                self.add_system_message(display);

                let tool_call = ToolCall {
                    id: Uuid::new_v4().to_string(),
                    session_id: self.session_id.clone(),
                    tool_name: name.clone(),
                    args: args.clone(),
                    result: result.clone(),
                    success,
                    created_at: Utc::now(),
                };
                let _ = self.memory.save_tool_call(&tool_call);
                if success {
                    self.tool_calls_count += 1;
                }

                // Track tool outcome for adaptive learning
                if let Some(ref evolution) = self.evolution {
                    evolution.track_tool_outcome(&name, success, 0);
                }

                // Estimate tool execution time from tool_calls_count change
                // (We don't have duration here, but we can track it in execute_approved_tool_task)
                self.model_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool result: {}", result),
                    images: None,
                });
            }
            StreamEvent::FollowUp(content) => {
                self.add_assistant_message(content);
            }
            StreamEvent::MultiModelResponse { name, content, metrics } => {
                if !content.is_empty() {
                    // Attach to the most recent assistant message
                    if let Some(last_idx) = self.messages.iter().rposition(|m| m.role == "assistant") {
                        self.messages[last_idx].multi_model_responses.push(SecondaryResponse {
                            model_name: name,
                            content,
                            latency_ms: metrics.total_latency_ms,
                            tokens: metrics.tokens_generated,
                        });
                    }
                }
            }
            StreamEvent::SetPendingSuggestion(suggestion) => {
                self.pending_suggestion = Some(suggestion);
                self.mode = AppMode::ToolApproval;
                self.tool_approval_shown_at = Some(Instant::now());
            }
            StreamEvent::Error(msg) => {
                self.is_streaming = false;
                self.add_system_message(msg);
            }
            StreamEvent::SystemMessage(msg) => {
                self.add_system_message(msg);
            }
            StreamEvent::Done => {
                self.is_streaming = false;
                self.stream_rx = None;
            }
        }
    }

    /// Get visible messages based on scroll.
    fn visible_messages(&self, height: usize) -> Vec<&ChatMessage> {
        let start = self.scroll;
        let end = (self.scroll + height).min(self.messages.len());
        if start < self.messages.len() {
            self.messages[start..end].iter().collect()
        } else {
            Vec::new()
        }
    }

    /// Scroll up in chat history.
    fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    /// Scroll down in chat history.
    fn scroll_down(&mut self, amount: usize) {
        let max_scroll = self.messages.len().saturating_sub(1);
        self.scroll = (self.scroll + amount).min(max_scroll);
    }

    /// Get session duration as formatted string.
    fn session_duration(&self) -> String {
        let elapsed = self.session_start.elapsed();
        let mins = elapsed.as_secs() / 60;
        let secs = elapsed.as_secs() % 60;
        if mins > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}s", secs)
        }
    }

    /// Estimate context used in tokens (rough word-count based).
    fn context_used(&self) -> usize {
        self.model_messages.iter()
            .map(|m| m.content.split_whitespace().count())
            .sum()
    }
}

pub async fn run(config: Config) -> Result<()> {
    // Initialize theme from config
    if let Some(theme) = crate::tui::theme::Theme::by_name(&config.theme) {
        crate::tui::theme::set_theme(theme);
    }

    let mut terminal = ratatui::init();
    terminal.clear()?;

    let mut app = App::new(config.clone())?;

    // Initialize MCP connections if enabled
    if config.gateway.mcp.enabled && !config.gateway.mcp.servers.is_empty() {
        app.init_mcp().await;
    }

    // Inject welcome message using the agent's configured identity
    let welcome = ascii_art::session_header(crate::VERSION);
    app.add_system_message(welcome);
    if !config.agent.greeting.is_empty() {
        app.add_system_message(config.agent.greeting.clone());
    }
    let mut last_tick = Instant::now();

    let result = run_app(&mut terminal, &mut app, &mut last_tick).await;

    // Cleanup MCP connections
    app.shutdown_mcp().await;

    ratatui::restore();
    result
}

async fn run_app(
    terminal: &mut Terminal<impl Backend>,
    app: &mut App,
    last_tick: &mut Instant,
) -> Result<()> {
    loop {
        terminal.draw(|f| draw_ui(f, app))?;

        // Drain any stream events from the background task before handling input.
        // Use Option::take() to avoid borrowing app.stream_rx while calling apply_stream_event.
        if let Some(mut rx) = app.stream_rx.take() {
            while let Ok(event) = rx.try_recv() {
                app.apply_stream_event(event);
            }
            app.stream_rx = Some(rx);
        }

        let timeout = TICK_RATE
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if handle_input(app, key).await? {
                    break;
                }
            }
        }

        if last_tick.elapsed() >= TICK_RATE {
            *last_tick = Instant::now();
        }

        // Auto-close tool approval popup after 60 seconds of inactivity
        if app.mode == AppMode::ToolApproval {
            if let Some(shown_at) = app.tool_approval_shown_at {
                if shown_at.elapsed() >= Duration::from_secs(60) {
                    let tool_name = app.pending_suggestion.as_ref().map(|s| s.tool_name.clone()).unwrap_or_default();
                    app.pending_suggestion = None;
                    app.mode = AppMode::Normal;
                    app.tool_approval_shown_at = None;
                    app.add_system_message(format!(
                        "⏭ Tool approval timed out after 60s{}.",
                        if tool_name.is_empty() { "".to_string() } else { format!(" for {}", tool_name) }
                    ));
                }
            }
        }

        if app.should_exit {
            break;
        }
    }

    Ok(())
}

async fn handle_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    // ToolApproval mode: handle y/n immediately, no other input accepted
    if app.mode == AppMode::ToolApproval {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(suggestion) = app.pending_suggestion.take() {
                    app.mode = AppMode::Normal;
                    app.add_system_message(format!(
                        "✅ Approved: {} {}",
                        suggestion.tool_name, suggestion.args
                    ));

                    // Spawn background task so follow-up responses are processed
                    // through the same event pipeline (handles chained tool calls)
                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    app.stream_rx = Some(rx);

                    let provider = app.provider.clone();
                    let model = app.model.clone();
                    let model_messages = app.model_messages.clone();
                    let security_engine = app.security_engine.clone();

                    tokio::spawn(async move {
                        let _ = execute_approved_tool_task(
                            tx,
                            provider,
                            model,
                            model_messages,
                            security_engine,
                            suggestion,
                        ).await;
                    });
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let tool_name = app.pending_suggestion.as_ref().map(|s| s.tool_name.clone()).unwrap_or_default();
                app.pending_suggestion = None;
                app.mode = AppMode::Normal;
                app.add_system_message(format!(
                    "⏭ Skipped tool suggestion{}.",
                    if tool_name.is_empty() { "".to_string() } else { format!(" for {}", tool_name) }
                ));
            }
            _ => {
                // Ignore all other keys in approval mode
                app.add_system_message("Press 'y' to approve or 'n' to skip.".to_string());
            }
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
            let now = Instant::now();
            let within_window = app.last_ctrl_c.map(|t| now.duration_since(t).as_secs() < 2).unwrap_or(false);
            
            if within_window {
                app.ctrl_c_count += 1;
            } else {
                app.ctrl_c_count = 1;
            }
            app.last_ctrl_c = Some(now);
            
            if app.ctrl_c_count >= 2 {
                return Ok(true);
            } else if !app.input.is_empty() {
                app.input.clear();
                app.cursor_position = 0;
                app.add_system_message("Input cleared. Press Ctrl+C again to quit.".to_string());
            } else {
                app.add_system_message("Press Ctrl+C again to quit.".to_string());
            }
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Ok(true);
        }
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.messages.clear();
            app.scroll = 0;
        }
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.sidebar_expanded = !app.sidebar_expanded;
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.show_model_selector();
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.autonomous_mode = !app.autonomous_mode;
            let status = if app.autonomous_mode {
                "🚀 AUTONOMOUS MODE ON — High-risk tools auto-approved (curl, ssh, redirects). sudo/sensitive paths still blocked."
            } else {
                "🔒 Autonomous mode off — Standard security (Medium risk threshold)."
            };
            app.add_system_message(status.to_string());
        }
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let names = crate::tui::theme::Theme::names();
            let current = crate::tui::theme::current_theme().name;
            let idx = names.iter().position(|n| n == &current).unwrap_or(0);
            let next_idx = (idx + 1) % names.len();
            let next_name = names[next_idx].clone();
            if let Some(theme) = crate::tui::theme::Theme::by_name(&next_name) {
                crate::tui::theme::set_theme(theme);
                app.add_system_message(format!("🎨 Theme: {}", next_name));
            }
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.sidebar_tab = (app.sidebar_tab + 1) % 3;
            app.sidebar_scroll = 0;
            let tab_name = match app.sidebar_tab {
                0 => "Tools",
                1 => "Skills",
                2 => "Swarm",
                _ => "Tools",
            };
            app.add_system_message(format!("📋 Sidebar: {}", tab_name));
        }
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.swarm_active = !app.swarm_active;
            if app.swarm_active {
                app.sidebar_tab = 2;
                app.add_system_message("🐝 Swarm mode active. Use /swarm init <prompt> to spawn agents.".to_string());
            } else {
                app.sidebar_tab = 0;
                app.add_system_message("🐝 Swarm mode deactivated.".to_string());
            }
        }
        KeyCode::Char('v') if key.modifiers == KeyModifiers::CONTROL => {
            match Clipboard::new().and_then(|mut cb| cb.get_text()) {
                Ok(text) => {
                    for ch in text.chars() {
                        app.input.insert(app.cursor_position, ch);
                        app.cursor_position += 1;
                    }
                }
                Err(e) => {
                    app.add_system_message(format!("⚠️ Paste failed: {}", e));
                }
            }
        }
        KeyCode::Enter => {
            let input = app.input.trim().to_string();
            if !input.is_empty() {
                app.input.clear();
                app.cursor_position = 0;
                process_user_input(app, input).await?;
            }
        }
        KeyCode::Char(c) => {
            app.input.insert(app.cursor_position, c);
            app.cursor_position += 1;
        }
        KeyCode::Backspace => {
            if app.cursor_position > 0 {
                app.cursor_position -= 1;
                app.input.remove(app.cursor_position);
            }
        }
        KeyCode::Delete => {
            if app.cursor_position < app.input.len() {
                app.input.remove(app.cursor_position);
            }
        }
        KeyCode::Left => {
            if app.cursor_position > 0 {
                app.cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            if app.cursor_position < app.input.len() {
                app.cursor_position += 1;
            }
        }
        KeyCode::Home => {
            app.cursor_position = 0;
        }
        KeyCode::End => {
            app.cursor_position = app.input.len();
        }
        KeyCode::Up => {
            if app.show_comparison {
                app.comparison_selected = app.comparison_selected.saturating_sub(1);
            } else if app.focused_pane == 0 {
                app.sidebar_scroll = app.sidebar_scroll.saturating_sub(1);
            } else {
                app.scroll_up(3);
            }
        }
        KeyCode::Down => {
            if app.show_comparison {
                // Find the assistant message with the most secondary responses
                let max_responses = app.messages.iter()
                    .filter(|m| m.role == "assistant")
                    .map(|m| m.multi_model_responses.len())
                    .max()
                    .unwrap_or(0);
                if max_responses > 0 {
                    app.comparison_selected = (app.comparison_selected + 1).min(max_responses.saturating_sub(1));
                }
            } else if app.focused_pane == 0 {
                app.sidebar_scroll += 1;
            } else {
                app.scroll_down(3);
            }
        }
        KeyCode::PageUp => {
            if app.focused_pane == 0 {
                app.sidebar_scroll = app.sidebar_scroll.saturating_sub(5);
            } else {
                app.scroll_up(10);
            }
        }
        KeyCode::PageDown => {
            if app.focused_pane == 0 {
                app.sidebar_scroll += 5;
            } else {
                app.scroll_down(10);
            }
        }
        KeyCode::Esc => {
            if app.show_comparison {
                app.show_comparison = false;
            } else {
                return Ok(true);
            }
        }
        _ => {}
    }

    Ok(false)
}

async fn process_user_input(app: &mut App, input: String) -> Result<()> {
    if input == "exit" || input == "quit" {
        app.should_exit = true;
        return Ok(());
    }

    if input == "help" {
        app.add_system_message(
            "OpenShark Commands\n\
            \n\
            Chat commands:\n\
            • help              — Show this help\n\
            • tools             — List available tools\n\
            • history           — Show chat history\n\
            • context           — Show current context\n\
            • clear             — Clear chat\n\
            • exit              — Exit OpenShark\n\
            \n\
            Model commands:\n\
            • /models           — List available models\n\
            • /model <name>     — Switch to model\n\
            • /multi            — Toggle multi-model mode\n\
            \n\
            Image commands:\n\
            • /image <path>     — Attach an image to your next message\n\
            \n\
            Branch commands:\n\
            • /branch <name>    — Create new branch\n\
            • /branches         — List branches\n\
            • /switch <index>   — Switch to branch\n\
            \n\
            Evolution commands:\n\
            • /evolution        — Show adaptive state\n\
            \n\
            Swarm commands:\n\
            • /swarm init <prompt> — Initialize agent swarm\n\
            • /swarm start      — Start autonomous loop\n\
            • /swarm stop       — Stop swarm\n\
            • /swarm status     — Show swarm status\n\
            \n\
            Keybindings:\n\
            • Ctrl+C            — Copy / Quit (double-tap)\n\
            • Ctrl+L            — Clear chat\n\
            • Ctrl+B            — Toggle sidebar\n\
            • Ctrl+P            — Model selector\n\
            • Ctrl+A            — Toggle autonomous mode\n\
            • Ctrl+T            — Cycle theme\n\
            • Ctrl+W            — Toggle swarm mode\n\
            • Ctrl+S            — Cycle sidebar tab\n\
            • ↑ / ↓             — Scroll\n\
            • PgUp / PgDn       — Fast scroll"
                .to_string(),
        );
        return Ok(());
    }

    if input == "/models" || input == "/model" {
        app.show_model_selector();
        return Ok(());
    }

    if input.starts_with("/model ") {
        let model_name = input[7..].trim();
        if let Err(e) = app.switch_model(model_name) {
            app.add_system_message(format!("Error: {}", e));
        }
        return Ok(());
    }

    if input.starts_with("/branch ") {
        let name = input[8..].trim();
        app.create_branch(name);
        return Ok(());
    }

    if input == "/branches" {
        app.list_branches();
        return Ok(());
    }

    if input.starts_with("/switch ") {
        if let Ok(index) = input[8..].trim().parse::<usize>() {
            if let Err(e) = app.switch_branch(index) {
                app.add_system_message(format!("Error: {}", e));
            }
        } else {
            app.add_system_message("Usage: /switch <branch_index>".to_string());
        }
        return Ok(());
    }

    if input == "/multi" {
        app.toggle_multi_model();
        return Ok(());
    }

    if input == "/evolution" {
        if let Some(ref evolution) = app.evolution {
            app.add_system_message(evolution.state_summary());
        } else {
            app.add_system_message("Evolution engine not initialized.".to_string());
        }
        return Ok(());
    }

    if input == "/swarm" || input.starts_with("/swarm ") {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts.get(1).map(|s| *s).unwrap_or("status");
        let prompt = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();

        match cmd {
            "init" => {
                if prompt.is_empty() {
                    app.add_system_message("Usage: /swarm init <seed prompt>".to_string());
                    app.add_system_message("Example: /swarm init Build a REST API with auth".to_string());
                } else if !app.config.swarm.enabled {
                    app.add_system_message("🐝 Swarm mode is disabled in config.".to_string());
                    app.add_system_message("Set [swarm] enabled = true in ~/.config/openshark/config.toml".to_string());
                } else {
                    let engine = crate::swarm::SwarmEngine::new(app.config.swarm.clone());
                    match engine.init(&prompt, &app.config).await {
                        Ok(()) => {
                            let agents = engine.agent_snapshot().await;
                            app.swarm_agents = agents.clone();
                            app.swarm_running = false;
                            app.add_system_message(format!("🐝 Swarm initialized with {} agents", agents.len()));
                            for agent in agents {
                                app.add_system_message(format!("  🐝 {} ({}) — {}", agent.name, agent.role.name, agent.status));
                            }
                            app.add_system_message("Run /swarm start to begin the autonomous loop.".to_string());
                            app.swarm = Some(engine);
                            app.swarm_active = true;
                            app.sidebar_tab = 2;
                        }
                        Err(e) => app.add_system_message(format!("❌ Swarm init failed: {}", e)),
                    }
                }
            }
            "start" => {
                if let Some(ref engine) = app.swarm {
                    match engine.start().await {
                        Ok(()) => app.add_system_message("🐝 Swarm loop started.".to_string()),
                        Err(e) => app.add_system_message(format!("❌ Swarm start failed: {}", e)),
                    }
                } else {
                    app.add_system_message("🐝 No swarm initialized. Run /swarm init <prompt> first.".to_string());
                }
            }
            "stop" => {
                if let Some(ref engine) = app.swarm {
                    match engine.stop().await {
                        Ok(()) => {
                            app.add_system_message("🐝 Swarm stopped.".to_string());
                            app.swarm = None;
                            app.swarm_active = false;
                        }
                        Err(e) => app.add_system_message(format!("❌ Swarm stop failed: {}", e)),
                    }
                } else {
                    app.add_system_message("🐝 No swarm running.".to_string());
                }
            }
            "status" => {
                if let Some(ref engine) = app.swarm {
                    let status = engine.status().await;
                    app.add_system_message(format!("{}", status));
                } else {
                    app.add_system_message("🐝 No swarm active.".to_string());
                    app.add_system_message(format!("Config: enabled={}, max_agents={}, roles={:?}",
                        app.config.swarm.enabled,
                        app.config.swarm.max_agents,
                        app.config.swarm.roles));
                }
            }
            _ => {
                app.add_system_message("🐝 Swarm Commands:".to_string());
                app.add_system_message("  /swarm init <prompt>  — Initialize swarm".to_string());
                app.add_system_message("  /swarm start          — Start autonomous loop".to_string());
                app.add_system_message("  /swarm stop           — Stop swarm".to_string());
                app.add_system_message("  /swarm status         — Show swarm status".to_string());
            }
        }
        return Ok(());
    }

    // ── Image Attachment Command ────────────────────────────────────────────
    if input.starts_with("/image ") {
        let path_str = input[7..].trim();
        let path = std::path::Path::new(path_str);
        match crate::image_utils::encode_image_to_data_url(path) {
            Ok(data_url) => {
                app.pending_image = Some(data_url);
                app.add_system_message(format!("📎 Image attached: {} (will be sent with your next message)", path.display()));
            }
            Err(e) => {
                app.add_system_message(format!("❌ Failed to encode image: {}", e));
            }
        }
        return Ok(());
    }

    if input == "tools" {
        let tools_list = get_tools()
            .iter()
            .map(|t| format!("{} - {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");
        app.add_system_message(format!("Available tools:\n{}", tools_list));
        return Ok(());
    }

    if input == "history" {
        match app.memory.get_session_messages(&app.session_id) {
            Ok(history) => {
                let history_text = history
                    .iter()
                    .map(|msg| {
                        format!(
                            "[{}] {}: {}",
                            msg.created_at.format("%H:%M:%S"),
                            msg.role,
                            &msg.content[..msg.content.len().min(60)]
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                app.add_system_message(format!("Session history:\n{}", history_text));
            }
            Err(e) => app.add_system_message(format!("Failed to load history: {}", e)),
        }
        return Ok(());
    }

    if input == "context" {
        let injector = ContextInjector::new(&app.memory);
        match injector.get_context_summary(&app.session_id) {
            Ok(summary) => app.add_system_message(format!("Context:\n{}", summary)),
            Err(e) => app.add_system_message(format!("Failed to get context: {}", e)),
        }
        return Ok(());
    }

    if input.starts_with("what did we do about ")
        || input.starts_with("how did we solve ")
        || input.starts_with("tell me about ")
    {
        let injector = ContextInjector::new(&app.memory);
        match injector.answer_natural_query(&input) {
            Ok(answer) => app.add_system_message(answer),
            Err(e) => app.add_system_message(format!("Failed to answer query: {}", e)),
        }
        return Ok(());
    }

    // ── Control Commands ────────────────────────────────────────────────────
    // Natural language control words that interrupt or modify agent behavior
    let input_lower = input.to_lowercase();
    let control_words = [
        ("stop", "⏹ Stopped."),
        ("wait", "⏸ Paused. Type 'continue' or 'go' to resume."),
        ("hold on", "⏸ Paused. Type 'continue' or 'go' to resume."),
        ("hold up", "⏸ Paused. Type 'continue' or 'go' to resume."),
        ("pause", "⏸ Paused. Type 'continue' or 'go' to resume."),
        ("cancel", "❌ Cancelled."),
        ("cancel that", "❌ Cancelled."),
        ("nevermind", "❌ Cancelled."),
        ("never mind", "❌ Cancelled."),
        ("abort", "❌ Aborted."),
        ("continue", "▶ Resuming."),
        ("go", "▶ Resuming."),
        ("proceed", "▶ Resuming."),
        ("carry on", "▶ Resuming."),
        ("status", "📊 Status check..."),
        ("what are you doing", "📊 Checking current operation..."),
        ("give me a status update", "📊 Status check..."),
        ("give me an update", "📊 Status check..."),
        ("whats going on", "📊 Checking current operation..."),
        ("what's going on", "📊 Checking current operation..."),
        ("update me", "📊 Status check..."),
    ];

    for (word, response) in &control_words {
        if input_lower == *word || input_lower.starts_with(&format!("{} ", word)) {
            app.add_system_message(response.to_string());
            return Ok(());
        }
    }

    app.add_user_message(input.clone());

    if input.starts_with("agent:") {
        let task = input[6..].trim();
        if task.is_empty() {
            app.add_system_message(
                "Please provide a task after 'agent:'. Example: agent: fix the bug in src/main.rs"
                    .to_string(),
            );
            return Ok(());
        }

        app.mode = AppMode::Agent;
        app.add_system_message(format!("🦈 Agent Mode: {}", task));

        let agent_config = AgentConfig::default();
        match Agent::new(agent_config, &app.config) {
            Ok(agent) => {
                match agent.run_task(task).await {
                    Ok(result) => {
                        let mut response = format!(
                            "Agent Result: {}\nMessage: {}\nIterations: {}",
                            if result.success {
                                "✅ Success"
                            } else {
                                "⚠️ Partial"
                            },
                            result.message,
                            result.total_iterations
                        );
                        for (i, step) in result.step_results.iter().enumerate() {
                            response.push_str(&format!(
                                "\n  Step {}: {} {} → verified={} ({} iter)",
                                i + 1,
                                step.step.tool_name,
                                step.step.args,
                                step.verified,
                                step.iterations
                            ));
                        }
                        app.add_assistant_message(response);
                    }
                    Err(e) => app.add_system_message(format!("Agent error: {}", e)),
                }
            }
            Err(e) => app.add_system_message(format!("Failed to initialize agent: {}", e)),
        }

        app.mode = AppMode::Normal;
        return Ok(());
    }

    if input.starts_with("TOOL:") {
        handle_user_tool_invocation(app, &input)?;
        return Ok(());
    }

    // Spawn model response in background so the user message appears immediately.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.stream_rx = Some(rx);

    // ── Phase 1: Enrich system prompt with memory + skills ────────────────
    let mut model_messages = app.model_messages.clone();

    // ── Phase 1b: Context compression if threshold exceeded ────────────────
    let compression_notice = if let Some(ref mut compressor) = app.compressor {
        let estimated = crate::memory::compression::estimate_tokens(&model_messages
        );
        if compressor.should_compress(estimated, app.model_context_length) {
            match compressor.compress(&mut model_messages, &app.provider) {
                Ok(true) => {
                    let stats = compressor.stats();
                    Some(format!(
                        "🗜 Context compressed: {} messages → summaries ({} compressions, ~{} tokens saved)",
                        stats.messages_summarized,
                        stats.compressions_done,
                        stats.tokens_saved
                    ))
                }
                Ok(false) => None,
                Err(e) => Some(format!(
                    "⚠️ Context compression failed: {}",
                    e
                )),
            }
        } else {
            None
        }
    } else {
        None
    };
    if let Some(notice) = compression_notice {
        app.add_system_message(notice);
    }

    if let Some(ref evolution) = app.evolution {
        let base_prompt = if let Some(first) = model_messages.first() {
            first.content.clone()
        } else {
            String::new()
        };
        let enriched = evolution.build_enriched_prompt(
            &base_prompt,
            &input,
            &app.session_id,
        );
        if !model_messages.is_empty() {
            model_messages[0].content = enriched;
        }
    }

    let provider = app.provider.clone();
    let model = app.model.clone();
    let model_config = app.model_config.clone();
    let is_multi_model = app.multi_model_mode;
    let config = app.config.clone();

    tokio::spawn(async move {
        let _ = stream_model_response_task(
            tx,
            provider,
            model,
            model_config,
            model_messages,
            is_multi_model,
            config,
        ).await;
    });

    Ok(())
}

/// Parse all explicit TOOL: invocations from text, anywhere in the response.
/// Handles both `TOOL:tool_name args` and `TOOL: tool_name args` (with space after colon).
fn parse_embedded_tools(text: &str) -> Vec<(String, String)> {
    let mut tools = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("TOOL:") {
            let rest = &trimmed[5..]; // after "TOOL:"
            let rest = rest.trim_start(); // handle "TOOL: fs cat" → "fs cat"
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if !parts.is_empty() && !parts[0].is_empty() {
                let tool_name = parts[0].trim().to_string();
                let args = parts.get(1).unwrap_or(&"").trim().to_string();
                tools.push((tool_name, args));
            }
        }
    }
    tools
}

/// Strip TOOL: lines from assistant content for display.
fn strip_tool_lines(text: &str) -> String {
    let mut result = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("TOOL:") {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
        }
    }
    result
}

/// Execute a chain of tools and stream follow-up via the event channel.
async fn execute_tool_chain(
    tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    provider: &Provider,
    model: &str,
    model_messages: &[Message],
    security_engine: &crate::security::SecurityEngine,
    tools: &[(String, String)],
    original_content: &str,
) -> Result<()> {
    if tools.is_empty() {
        return Ok(());
    }

    let mut follow_messages = model_messages.to_vec();
    // Include the assistant's original message (with tool lines stripped for context)
    follow_messages.push(Message {
        role: "assistant".to_string(),
        content: strip_tool_lines(original_content),
        images: None,
    });

    let executor = AsyncToolExecutor::new();

    for (tool_name, args) in tools {
        // SECURITY GATE
        match security_engine.check_tool_call(tool_name, args) {
            crate::security::SecurityDecision::Allow => {}
            crate::security::SecurityDecision::RequireApproval { reason, risk_level } => {
                let _ = tx.send(StreamEvent::Error(format!(
                    "🔒 Security: Tool '{}' requires approval\n  Reason: {}\n  Risk: {:?}",
                    tool_name, reason, risk_level
                )));
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }
            crate::security::SecurityDecision::Deny { reason } => {
                let _ = tx.send(StreamEvent::Error(format!(
                    "🚫 Security: Tool '{}' blocked\n  Reason: {}",
                    tool_name, reason
                )));
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }
        }

        match executor
            .execute_with_timeout_simple(tool_name.clone(), args.clone(), 30000)
            .await
        {
            Ok(result) => {
                let sanitized = security_engine.sanitize_output(tool_name, &result);
                let _ = tx.send(StreamEvent::ToolResult {
                    name: tool_name.clone(),
                    args: args.clone(),
                    result: sanitized.clone(),
                    success: true,
                });
                follow_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool result ({} {}): {}", tool_name, args, sanitized),
                    images: None,
                });
            }
            Err(e) => {
                let _ = tx.send(StreamEvent::ToolResult {
                    name: tool_name.clone(),
                    args: args.clone(),
                    result: e.to_string(),
                    success: false,
                });
                follow_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool error ({} {}): {}", tool_name, args, e),
                    images: None,
                });
            }
        }
    }

    // Follow-up request with all tool results
    let follow_up = ChatRequest::new(model.to_string(), follow_messages, true);
    match provider.chat_stream(follow_up).await {
        Ok((follow_chunks, _metrics)) => {
            let follow_content: String = follow_chunks.join("");
            let _ = tx.send(StreamEvent::FollowUp(follow_content));
            let _ = tx.send(StreamEvent::Done);
        }
        Err(e) => {
            let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
            let _ = tx.send(StreamEvent::Done);
        }
    }

    Ok(())
}

/// Background task: call the model API and send events back to the TUI loop.
async fn stream_model_response_task(
    tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    provider: Provider,
    model: String,
    model_config: Option<crate::config::ModelConfig>,
    model_messages: Vec<Message>,
    is_multi_model: bool,
    config: Config,
) -> Result<()> {
    let _ = tx.send(StreamEvent::Start);

    // Create security engine for this task
    let security_engine = match crate::security::SecurityEngine::new(
        crate::security::SecurityConfig::load().unwrap_or_default()
    ) {
        Ok(engine) => engine,
        Err(e) => {
            let _ = tx.send(StreamEvent::Error(format!("Security engine init failed: {}", e)));
            return Ok(());
        }
    };

    let mut request = ChatRequest::new(model.clone(), model_messages.clone(), true);
    if let Some(ref model_config) = model_config {
        request.max_tokens = Some(model_config.context_length as u32);
    }

    let secondary_providers: Vec<(String, Provider)> = if is_multi_model {
        config.providers.iter()
            .filter(|(name, _)| **name != "kimi")
            .map(|(name, provider_cfg)| {
                (name.clone(), Provider::new(
                    name.clone(),
                    provider_cfg.base_url.clone(),
                    provider_cfg.api_key.clone(),
                    provider_cfg.kind.clone(),
                    provider_cfg.headers.clone(),
                ))
            })
            .collect()
    } else {
        Vec::new()
    };

    match provider.chat_stream(request).await {
        Ok((chunks, metrics)) => {
            let mut full_content = String::new();
            for chunk in &chunks {
                full_content.push_str(chunk);
                let _ = tx.send(StreamEvent::Chunk(chunk.clone()));
            }

            let _ = tx.send(StreamEvent::ResponseComplete {
                content: full_content.clone(),
                metrics,
            });

            // Handle tool invocation + follow-up
            // First, check for embedded TOOL: lines anywhere in the response
            let embedded_tools = parse_embedded_tools(&full_content);
            if !embedded_tools.is_empty() {
                let _ = execute_tool_chain(
                    &tx, &provider, &model, &model_messages, &security_engine, &embedded_tools, &full_content
                ).await;
            } else {
                // ── Handle natural-language tool suggestions ─────────────────────
                // If the model didn't output TOOL:... but its response contains a
                // high-confidence tool suggestion, execute it and follow up.
                let suggestions = crate::tools::detect_tool_suggestions(&full_content);
                if let Some(suggestion) = suggestions.into_iter().find(|s| s.confidence >= 0.6) {
                    // SECURITY GATE
                    match security_engine.check_tool_call(&suggestion.tool_name, &suggestion.args) {
                        crate::security::SecurityDecision::Allow => {
                            let _ = tx.send(StreamEvent::SystemMessage(format!(
                                "🔧 Auto-executing: {} {} (low risk)",
                                suggestion.tool_name, suggestion.args
                            )));

                            let executor = AsyncToolExecutor::new();
                            match executor
                                .execute_with_timeout_simple(
                                    suggestion.tool_name.clone(),
                                    suggestion.args.clone(),
                                    30000,
                                )
                                .await
                            {
                                Ok(result) => {
                                    let sanitized = security_engine.sanitize_output(&suggestion.tool_name, &result);
                                    let _ = tx.send(StreamEvent::ToolResult {
                                        name: suggestion.tool_name.clone(),
                                        args: suggestion.args.clone(),
                                        result: sanitized.clone(),
                                        success: true,
                                    });

                                    // Follow-up request with tool result
                                    let mut follow_messages = model_messages.clone();
                                    follow_messages.push(Message {
                                        role: "assistant".to_string(),
                                        content: full_content.clone(),
                                        images: None,
                                    });
                                    follow_messages.push(Message {
                                        role: "user".to_string(),
                                        content: format!("Tool result: {}", sanitized),
                                        images: None,
                                    });

                                    let follow_up = ChatRequest::new(
                                        model.clone(),
                                        follow_messages,
                                        true,
                                    );

                                    match provider.chat_stream(follow_up).await {
                                        Ok((follow_chunks, _metrics)) => {
                                            let follow_content: String = follow_chunks.join("");
                                            let _ = tx.send(StreamEvent::FollowUp(follow_content));
                                            let _ = tx.send(StreamEvent::Done);
                                        }
                                        Err(e) => {
                                            let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
                                            let _ = tx.send(StreamEvent::Done);
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(StreamEvent::ToolResult {
                                        name: suggestion.tool_name,
                                        args: suggestion.args,
                                        result: e.to_string(),
                                        success: false,
                                    });
                                    let _ = tx.send(StreamEvent::Done);
                                }
                            }
                        }
                        crate::security::SecurityDecision::RequireApproval { reason, risk_level } => {
                            let _ = tx.send(StreamEvent::Error(format!(
                                "🔒 Security: Tool '{}' requires approval\n  Reason: {}\n  Risk: {:?}",
                                suggestion.tool_name, reason, risk_level
                            )));
                            let _ = tx.send(StreamEvent::Done);
                        }
                        crate::security::SecurityDecision::Deny { reason } => {
                            let _ = tx.send(StreamEvent::Error(format!(
                                "🚫 Security: Tool '{}' blocked\n  Reason: {}",
                                suggestion.tool_name, reason
                            )));
                            let _ = tx.send(StreamEvent::Done);
                        }
                    }
                }
            }
        }
        Err(e) => {
            let error_msg = format!("{}", e);
            let display_msg = if let Some(json_start) = error_msg.find('{') {
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&error_msg[json_start..]) {
                    if let Some(msg) = json_val
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                    {
                        format!("API Error: {}", msg)
                    } else if let Some(msg) = json_val.get("message").and_then(|m| m.as_str()) {
                        format!("API Error: {}", msg)
                    } else {
                        error_msg
                    }
                } else {
                    error_msg
                }
            } else {
                error_msg
            };
            let _ = tx.send(StreamEvent::Error(display_msg));
        }
    }

    if is_multi_model {
        for (name, sec_provider) in secondary_providers {
            let req = ChatRequest::new(
                model.clone(),
                model_messages.clone(),
                true,
            );
            match sec_provider.chat_stream(req).await {
                Ok((chunks, metrics)) => {
                    let content: String = chunks.join("");
                    if !content.is_empty() {
                        let _ = tx.send(StreamEvent::MultiModelResponse {
                            name,
                            content,
                            metrics,
                        });
                    }
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(format!("[{}] Error: {}", name, e)));
                }
            }
        }
    }

    let _ = tx.send(StreamEvent::Done);
    Ok(())
}

fn handle_user_tool_invocation(app: &mut App, input: &str) -> Result<()> {
    let rest = &input[5..];
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if parts.is_empty() {
        return Ok(());
    }

    let tool_name = parts[0];
    let args = parts.get(1).unwrap_or(&"");

    // SECURITY GATE: Check tool call before execution
    match app.security_engine.check_tool_call(tool_name, args) {
        crate::security::SecurityDecision::Allow => {
            // Proceed with execution
        }
        crate::security::SecurityDecision::RequireApproval { reason, risk_level } => {
            app.add_system_message(format!(
                "🔒 Security: Tool '{}' requires approval\n  Reason: {}\n  Risk: {:?}\n  Use 'y' to approve or 'n' to deny",
                tool_name, reason, risk_level
            ));
            // Store pending approval state could be added here
            return Ok(());
        }
        crate::security::SecurityDecision::Deny { reason } => {
            app.add_system_message(format!(
                "🚫 Security: Tool '{}' blocked\n  Reason: {}",
                tool_name, reason
            ));
            app.security_engine.audit(tool_name, args, false, crate::security::RiskLevel::Critical, &reason);
            return Ok(());
        }
    }

    let found_tool = find_tool(tool_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", tool_name))?;

    app.add_system_message(format!("🔧 Using tool: {}", tool_name));

    match found_tool.execute(args) {
        Ok(result) => {
            let sanitized = app.security_engine.sanitize_output(tool_name, &result);
            app.add_system_message(format!("Result: {}", &sanitized[..sanitized.len().min(500)]));

            let tool_call = ToolCall {
                id: Uuid::new_v4().to_string(),
                session_id: app.session_id.clone(),
                tool_name: tool_name.to_string(),
                args: args.to_string(),
                result: sanitized.clone(),
                success: true,
                created_at: Utc::now(),
            };
            let _ = app.memory.save_tool_call(&tool_call);
            app.tool_calls_count += 1;
            app.security_engine.audit(tool_name, args, true, crate::security::RiskLevel::Low, "approved");

            app.model_messages.push(Message {
                role: "user".to_string(),
                content: format!("Tool {} returned: {}", tool_name, sanitized),
                images: None,
            });
        }
        Err(e) => {
            app.add_system_message(format!("Tool error: {}", e));
            let tool_call = ToolCall {
                id: Uuid::new_v4().to_string(),
                session_id: app.session_id.clone(),
                tool_name: tool_name.to_string(),
                args: args.to_string(),
                result: e.to_string(),
                success: false,
                created_at: Utc::now(),
            };
            let _ = app.memory.save_tool_call(&tool_call);
            app.security_engine.audit(tool_name, args, false, crate::security::RiskLevel::High, &e.to_string());
        }
    }

    Ok(())
}

async fn execute_tool_suggestion(app: &mut App, suggestion: &ToolSuggestion) -> Result<()> {
    // SECURITY GATE: Check before executing suggested tool
    match app.security_engine.check_tool_call(&suggestion.tool_name, &suggestion.args) {
        crate::security::SecurityDecision::Allow => {}
        crate::security::SecurityDecision::RequireApproval { reason, risk_level } => {
            app.add_system_message(format!(
                "🔒 Security: Suggested tool '{}' requires approval\n  Reason: {}\n  Risk: {:?}",
                suggestion.tool_name, reason, risk_level
            ));
            return Ok(());
        }
        crate::security::SecurityDecision::Deny { reason } => {
            app.add_system_message(format!(
                "🚫 Security: Suggested tool '{}' blocked\n  Reason: {}",
                suggestion.tool_name, reason
            ));
            app.security_engine.audit(&suggestion.tool_name, &suggestion.args, false,
                crate::security::RiskLevel::Critical, &reason
            );
            return Ok(());
        }
    }

    let executor = AsyncToolExecutor::new();
    match executor
        .execute_with_timeout(
            suggestion.tool_name.clone(),
            suggestion.args.clone(),
            30000,
        )
        .await
    {
        Ok((result, metrics)) => {
            let _ = app.memory.save_performance_metric(
                "tool_execution",
                &metrics.tool_name,
                metrics.duration_ms,
                Some(&format!("success={}", metrics.success)),
            );

            let sanitized = app.security_engine.sanitize_output(&suggestion.tool_name, &result);
            app.add_system_message(format!(
                "Result: {} ({}ms)",
                &sanitized[..sanitized.len().min(200)],
                metrics.duration_ms
            ));

            let tool_call = ToolCall {
                id: Uuid::new_v4().to_string(),
                session_id: app.session_id.clone(),
                tool_name: suggestion.tool_name.clone(),
                args: suggestion.args.clone(),
                result: sanitized.clone(),
                success: true,
                created_at: Utc::now(),
            };
            let _ = app.memory.save_tool_call(&tool_call);
            app.tool_calls_count += 1;
            app.security_engine.audit(
                &suggestion.tool_name, &suggestion.args, true,
                crate::security::RiskLevel::Low, "approved"
            );

            app.model_messages.push(Message {
                role: "assistant".to_string(),
                content: format!("TOOL:{} {}", suggestion.tool_name, suggestion.args),
                images: None,
            });
            app.model_messages.push(Message {
                role: "user".to_string(),
                content: format!("Tool result: {}", sanitized),
                images: None,
            });

            let follow_up = ChatRequest::new(
                app.model.clone(),
                app.model_messages.clone(),
                true,
            );

            match app.provider.chat_stream(follow_up).await {
                Ok((chunks, _metrics)) => {
                    let mut follow_content = String::new();
                    for chunk in chunks {
                        follow_content.push_str(&chunk);
                    }
                    app.add_assistant_message(follow_content);
                }
                Err(e) => app.add_system_message(format!("Follow-up failed: {}", e)),
            }
        }
        Err(e) => {
            app.add_system_message(format!("Tool execution failed: {}", e));
            app.security_engine.audit(
                &suggestion.tool_name, &suggestion.args, false,
                crate::security::RiskLevel::High, &e.to_string()
            );
        }
    }

    Ok(())
}

/// Background task: execute an approved tool suggestion and stream events back.
/// This mirrors stream_model_response_task so follow-ups with tool suggestions
/// are handled through the same pipeline (enabling chained tool calls).
async fn execute_approved_tool_task(
    tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    provider: Provider,
    model: String,
    model_messages: Vec<Message>,
    security_engine: crate::security::SecurityEngine,
    suggestion: ToolSuggestion,
) -> Result<()> {
    let executor = AsyncToolExecutor::new();
    match executor
        .execute_with_timeout_simple(
            suggestion.tool_name.clone(),
            suggestion.args.clone(),
            30000,
        )
        .await
    {
        Ok(result) => {
            let sanitized = security_engine.sanitize_output(&suggestion.tool_name, &result);
            let _ = tx.send(StreamEvent::ToolResult {
                name: suggestion.tool_name.clone(),
                args: suggestion.args.clone(),
                result: sanitized.clone(),
                success: true,
            });

            // Follow-up request with tool result
            let mut follow_messages = model_messages.clone();
            follow_messages.push(Message {
                role: "assistant".to_string(),
                content: format!("TOOL:{} {}", suggestion.tool_name, suggestion.args),
                images: None,
            });
            follow_messages.push(Message {
                role: "user".to_string(),
                content: format!("Tool result: {}", sanitized),
                images: None,
            });

            let follow_up = ChatRequest::new(
                model.clone(),
                follow_messages.clone(),
                true,
            );

            match provider.chat_stream(follow_up).await {
                Ok((chunks, metrics)) => {
                    let follow_content: String = chunks.join("");
                    let _ = tx.send(StreamEvent::ResponseComplete {
                        content: follow_content.clone(),
                        metrics,
                    });

                    // ── Handle chained tool suggestions in follow-up ──────────
                    if !follow_content.starts_with("TOOL:") {
                        let suggestions = crate::tools::detect_tool_suggestions(&follow_content);
                        if let Some(next) = suggestions.into_iter().find(|s| s.confidence >= 0.6) {
                            let next_tool = next.tool_name.clone();
                            let next_args = next.args.clone();
                            match security_engine.check_tool_call(&next_tool, &next_args
                            ) {
                                crate::security::SecurityDecision::Allow => {
                                    let _ = tx.send(StreamEvent::SystemMessage(format!(
                                        "🔧 Auto-executing: {} {} (low risk)",
                                        next_tool, next_args
                                    )));
                                    // Recurse through same pipeline
                                    let _ = executor
                                        .execute_with_timeout_simple(
                                            next_tool,
                                            next_args,
                                            30000,
                                        )
                                        .await;
                                }
                                crate::security::SecurityDecision::RequireApproval { reason: _, risk_level } => {
                                    let _ = tx.send(StreamEvent::SetPendingSuggestion(next));
                                    let _ = tx.send(StreamEvent::SystemMessage(format!(
                                        "🔒 Tool '{}' requires approval (risk: {:?}) — press y/n",
                                        next_tool, risk_level
                                    )));
                                }
                                crate::security::SecurityDecision::Deny { reason } => {
                                    let _ = tx.send(StreamEvent::Error(format!(
                                        "🚫 Security: Tool '{}' blocked\n  Reason: {}",
                                        next_tool, reason
                                    )));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
                }
            }
        }
        Err(e) => {
            let _ = tx.send(StreamEvent::ToolResult {
                name: suggestion.tool_name,
                args: suggestion.args,
                result: e.to_string(),
                success: false,
            });
        }
    }

    let _ = tx.send(StreamEvent::Done);
    Ok(())
}

fn detect_high_confidence_suggestion(content: &str) -> Option<ToolSuggestion> {
    let suggestions = detect_tool_suggestions(content);
    suggestions.into_iter().find(|s| s.confidence >= 0.6)
}

// ---------------------------------------------------------------------------
// UI Drawing
// ---------------------------------------------------------------------------

fn draw_ui(f: &mut Frame, app: &App) {
    let size = f.area();

    let main_layout = if app.sidebar_expanded {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
            .split(size)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(0), Constraint::Percentage(100)])
            .split(size)
    };

    if app.sidebar_expanded {
        draw_sidebar(f, app, main_layout[0]);
    }

    let chat_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(input_bar_height(app, main_layout[1].width))])
        .split(main_layout[1]);

    draw_chat_area(f, app, chat_layout[0]);
    draw_input_bar(f, app, chat_layout[1]);

    if app.mode == AppMode::ToolApproval {
        draw_tool_approval_popup(f, app);
    }

    if app.show_comparison {
        draw_comparison_overlay(f, app);
    }
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    // Single outer border for the whole sidebar — no nested boxes
    let sidebar_block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style())
        .style(bg_style());

    let inner = sidebar_block.inner(area);
    f.render_widget(sidebar_block, area);

    // Compact vertical layout: header → session → shortcuts → tools/skills → perf
    let sidebar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Compact logo + tagline
            Constraint::Length(9),  // Session info (7 lines + padding)
            Constraint::Length(9),  // Shortcuts (7 lines + padding)
            Constraint::Length(8),  // Tools/Skills (up to 6 with tab header)
            Constraint::Min(3),     // Performance (flexible)
        ])
        .split(inner);

    // Compact header: harness name + version (hardcoded, separate from agent identity)
    let mut header_lines = vec![
        Line::from(vec![
            Span::styled("🦈 ", shark_style()),
            Span::styled("openshark", highlight_style()),
            Span::styled(format!(" v{}", crate::VERSION), muted_style()),
        ]),
    ];
    if !app.config.agent.tagline.is_empty() {
        header_lines.push(Line::from(vec![
            Span::styled(app.config.agent.tagline.clone(), muted_style()),
        ]));
    }
    let header = Paragraph::new(Text::from(header_lines))
        .alignment(Alignment::Center)
        .style(bg_style());
    f.render_widget(header, sidebar_layout[0]);

    // Session info — no inner border, just styled text with section header
    let ctx_used = app.context_used();
    let ctx_pct = if app.model_context_length > 0 {
        (ctx_used * 100 / app.model_context_length).min(100)
    } else { 0 };
    let ctx_color = if ctx_pct > 80 { error_style() } else if ctx_pct > 50 { accent_style() } else { text_style() };

    let session_info = vec![
        Line::from(vec![
            Span::styled("Session  ", muted_style()),
            Span::styled(&app.session_id[..8.min(app.session_id.len())], highlight_style()),
        ]),
        Line::from(vec![
            Span::styled("Model    ", muted_style()),
            Span::styled(&app.model, accent_style()),
        ]),
        Line::from(vec![
            Span::styled("Max Ctx  ", muted_style()),
            Span::styled(format!("{}", app.model_context_length), text_style()),
        ]),
        Line::from(vec![
            Span::styled("Ctx Used ", muted_style()),
            Span::styled(format!("{} ({}%)", ctx_used, ctx_pct), ctx_color),
        ]),
        Line::from(vec![
            Span::styled("Duration ", muted_style()),
            Span::styled(app.session_duration(), text_style()),
        ]),
        Line::from(vec![
            Span::styled("Tokens   ", muted_style()),
            Span::styled(app.tokens_used.to_string(), text_style()),
        ]),
        Line::from(vec![
            Span::styled("Tools    ", muted_style()),
            Span::styled(app.tool_calls_count.to_string(), text_style()),
        ]),
    ];
    let session = Paragraph::new(Text::from(session_info))
        .block(
            Block::default()
                .title(" Session ")
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(session, sidebar_layout[1]);

    // Shortcuts — clean two-column layout
    let shortcuts = vec![
        Line::from(vec![Span::styled("Ctrl+C×2", accent_style()), Span::styled(" Quit", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+L  ", accent_style()), Span::styled("Clear chat", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+B  ", accent_style()), Span::styled("Toggle sidebar", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+P  ", accent_style()), Span::styled("Model selector", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+A  ", accent_style()), Span::styled("Autonomous mode", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+T  ", accent_style()), Span::styled("Cycle theme", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+S  ", accent_style()), Span::styled("Tools/Skills", muted_style())]),
        Line::from(vec![Span::styled("↑/↓     ", accent_style()), Span::styled("Scroll", muted_style())]),
        Line::from(vec![Span::styled("PgUp/Dn ", accent_style()), Span::styled("Fast scroll", muted_style())]),
    ];
    let shortcuts_para = Paragraph::new(Text::from(shortcuts))
        .block(
            Block::default()
                .title(" Shortcuts ")
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(shortcuts_para, sidebar_layout[2]);

    // Tools / Skills — tabbed view with scrolling
    let (tab_title, tab_items): (String, Vec<Line>) = if app.sidebar_tab == 0 {
        let all_tools = get_tools();
        let tools: Vec<Line> = all_tools
            .iter()
            .skip(app.sidebar_scroll)
            .take(6)
            .map(|t| {
                let desc = t.description();
                let desc_short = &desc[..desc.len().min(22)];
                Line::from(vec![
                    Span::styled(format!("{:<10}", t.name()), tool_style()),
                    Span::styled(desc_short.to_string(), muted_style()),
                ])
            })
            .collect();
        (format!(" Tools [{}] ", all_tools.len()), tools)
    } else if app.sidebar_tab == 1 {
        let skills: Vec<Line> = app.skill_registry.as_ref()
            .map(|reg| reg.all_skills().iter()
                .skip(app.sidebar_scroll)
                .take(6)
                .map(|skill| {
                    let desc = &skill.description;
                    let desc_short = &desc[..desc.len().min(22)];
                    Line::from(vec![
                        Span::styled(format!("{:<10}", &skill.name), tool_style()),
                        Span::styled(desc_short.to_string(), muted_style()),
                    ])
                })
                .collect()
            )
            .unwrap_or_else(|| vec![
                Line::from(vec![Span::styled("No skills loaded", muted_style())]),
                Line::from(vec![Span::styled("Add .md files to ~/.config/openshark/skills/", muted_style())]),
            ]);
        let count = app.skill_registry.as_ref().map(|r| r.all_skills().len()).unwrap_or(0);
        (format!(" Skills [{}] ", count), skills)
    } else {
        // Swarm tab
        let swarm_lines: Vec<Line> = if !app.swarm_agents.is_empty() {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Status: ", muted_style()),
                    Span::styled(if app.swarm_running { "🟢 Running" } else { "⏹ Idle" }, text_style()),
                ]),
                Line::from(vec![
                    Span::styled("Agents: ", muted_style()),
                    Span::styled(format!("{}", app.swarm_agents.len()), text_style()),
                ]),
                Line::from(vec![]),
            ];
            for agent in app.swarm_agents.iter().skip(app.sidebar_scroll).take(6) {
                let status_icon = match agent.status {
                    crate::swarm::AgentStatus::Idle => "⏸",
                    crate::swarm::AgentStatus::Working { .. } => "🟡",
                    crate::swarm::AgentStatus::Reviewing { .. } => "👁",
                    crate::swarm::AgentStatus::WaitingForConsensus { .. } => "⏳",
                    crate::swarm::AgentStatus::Error { .. } => "❌",
                    crate::swarm::AgentStatus::Completed { .. } => "✅",
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", status_icon), text_style()),
                    Span::styled(format!("{:<10}", agent.name), tool_style()),
                    Span::styled(format!("cycles:{}", agent.cycles_completed), muted_style()),
                ]));
            }
            lines
        } else {
            vec![
                Line::from(vec![Span::styled("No swarm active", muted_style())]),
                Line::from(vec![Span::styled("Ctrl+W to activate", muted_style())]),
                Line::from(vec![]),
                Line::from(vec![Span::styled("Commands:", muted_style())]),
                Line::from(vec![Span::styled("/swarm init <prompt>", accent_style())]),
                Line::from(vec![Span::styled("/swarm start", accent_style())]),
                Line::from(vec![Span::styled("/swarm stop", accent_style())]),
                Line::from(vec![Span::styled("/swarm status", accent_style())]),
            ]
        };
        (" Swarm ".to_string(), swarm_lines)
    };

    let tools_para = Paragraph::new(Text::from(tab_items))
        .block(
            Block::default()
                .title(tab_title)
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(tools_para, sidebar_layout[3]);

    // Performance — per-session metrics
    let perf_lines = if app.session_perf.requests > 0 {
        vec![
            Line::from(vec![
                Span::styled("First token: ", muted_style()),
                Span::styled(format!("{}ms", app.session_perf.avg_first_token()), text_style()),
            ]),
            Line::from(vec![
                Span::styled("Total latency: ", muted_style()),
                Span::styled(format!("{}ms", app.session_perf.avg_total_latency()), text_style()),
            ]),
            Line::from(vec![
                Span::styled("Tool exec: ", muted_style()),
                Span::styled(format!("{}ms", app.session_perf.avg_tool_exec()), text_style()),
            ]),
            Line::from(vec![
                Span::styled("Requests: ", muted_style()),
                Span::styled(app.session_perf.requests.to_string(), text_style()),
            ]),
        ]
    } else {
        vec![
            Line::from(vec![Span::styled("No performance data yet", muted_style())]),
            Line::from(vec![Span::styled("Start chatting to collect metrics", muted_style())]),
        ]
    };
    let perf = Paragraph::new(Text::from(perf_lines))
        .block(
            Block::default()
                .title(" Performance ")
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(perf, sidebar_layout[4]);
}

fn draw_chat_area(f: &mut Frame, app: &App, area: Rect) {
    let chat_block = Block::default()
        .title(" Chat ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(if app.focused_pane == 1 {
            focused_border_style()
        } else {
            border_style()
        })
        .style(bg_style());

    let inner = chat_block.inner(area);
    f.render_widget(chat_block, area);

    let visible_height = inner.height as usize;
    let visible = app.visible_messages(visible_height);

    let mut lines: Vec<Line> = Vec::new();

    for msg in visible {
        let user_name = if app.config.user_name.is_empty() {
            "user"
        } else {
            &app.config.user_name
        };
        let agent_name = &app.config.agent.display_name;

        let (role_style, content_style, prefix, display_role) = match msg.role.as_str() {
            "user" => (accent_style(), text_style(), "❯ ".to_string(), user_name.to_string()),
            "assistant" => {
                let agent_emoji = if app.config.agent.emoji.is_empty() {
                    "🦈"
                } else {
                    &app.config.agent.emoji
                };
                (shark_style(), text_style(), format!("{} ", agent_emoji), agent_name.to_string())
            }
            "system" => {
                // Use error styling for error messages
                let is_error = msg.content.contains("Error:")
                    || msg.content.contains("error:")
                    || msg.content.contains("Failed")
                    || msg.content.contains("failed");
                if is_error {
                    (error_style(), error_style(), "⚠ ".to_string(), "system".to_string())
                } else {
                    (muted_style(), muted_style(), "ℹ ".to_string(), "system".to_string())
                }
            }
            _ => (text_style(), text_style(), "  ".to_string(), msg.role.clone()),
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, role_style),
            Span::styled(display_role, role_style.add_modifier(Modifier::BOLD)),
        ]));

        // Image attachment indicator
        if msg.images.is_some() {
            lines.push(Line::from(vec![
                Span::styled(
                    "  📎 Image attached".to_string(),
                    muted_style().add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        for content_line in msg.content.lines() {
            // Welcome logo lines use purple for visibility against dark bg
            let line_style = if msg.role == "system" && content_line.contains('█') {
                Style::default().fg(current_theme().border_unfocused).add_modifier(Modifier::BOLD)
            } else {
                content_style
            };
            lines.push(Line::from(vec![Span::styled(
                content_line,
                line_style,
            )]));
        }

        // Multi-model response indicator on assistant messages
        if msg.role == "assistant" && !msg.multi_model_responses.is_empty() {
            let count = msg.multi_model_responses.len();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("📊 {} alternate response{} — Ctrl+V to compare", count, if count == 1 { "" } else { "s" }),
                    muted_style().add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        lines.push(Line::from(""));
    }

    if app.is_streaming {
        let agent_name = &app.config.agent.display_name;
        let agent_emoji = if app.config.agent.emoji.is_empty() {
            "🦈"
        } else {
            &app.config.agent.emoji
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", agent_emoji), shark_style()),
            Span::styled(agent_name, shark_style().add_modifier(Modifier::BOLD)),
        ]));
        for line in app.streaming_content.lines() {
            lines.push(Line::from(vec![Span::styled(line, text_style())]));
        }
        lines.push(Line::from(vec![Span::styled("▌", accent_style())]));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: true })
        .style(bg_style());

    f.render_widget(paragraph, inner);

    if app.messages.len() > visible_height {
        let mut scrollbar_state = ScrollbarState::new(app.messages.len())
            .position(app.scroll)
            .viewport_content_length(visible_height);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(accent_style())
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        f.render_stateful_widget(
            scrollbar,
            inner.inner(Margin {
                vertical: 0,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

fn draw_input_bar(f: &mut Frame, app: &App, area: Rect) {
    let input_block = Block::default()
        .title(" Input ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(if app.focused_pane == 2 {
            focused_border_style()
        } else {
            border_style()
        })
        .style(bg_style());

    let inner = input_block.inner(area);
    f.render_widget(input_block, area);

    let input_text = if app.input.is_empty() {
        if app.mode == AppMode::ToolApproval {
            "Tool suggestion pending. Press 'y' to execute, 'n' to skip."
        } else if app.is_streaming {
            "Streaming response..."
        } else {
            "Type a message or command..."
        }
    } else {
        &app.input
    };

    let style = if app.input.is_empty() {
        muted_style()
    } else {
        text_style()
    };

    let paragraph = Paragraph::new(input_text)
        .style(style)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, inner);

    if !app.input.is_empty() {
        // Calculate cursor position accounting for line wrapping
        let available_width = inner.width as usize;
        let (cursor_x, cursor_y) = compute_wrapped_cursor_position(
            &app.input,
            app.cursor_position,
            available_width,
            inner.x,
            inner.y,
        );
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Compute the visual height the input bar needs based on text length and wrap width.
fn input_bar_height(app: &App, area_width: u16) -> u16 {
    let available_width = area_width.saturating_sub(2).max(1) as usize; // minus borders
    let text = if app.input.is_empty() {
        // Placeholder text length
        "Type a message or command...".len()
    } else {
        app.input.len()
    };
    let lines = (text + available_width - 1) / available_width; // ceil division
    let lines = lines.max(1);
    // Cap at 8 lines so it doesn't eat the whole chat area
    let capped = lines.min(8);
    (capped as u16) + 2 // +2 for borders
}

/// Compute the actual screen (x, y) for the cursor given a text buffer,
/// a cursor byte/char position, and the available wrap width.
fn compute_wrapped_cursor_position(
    text: &str,
    cursor_pos: usize,
    wrap_width: usize,
    base_x: u16,
    base_y: u16,
) -> (u16, u16) {
    if text.is_empty() || cursor_pos == 0 {
        return (base_x, base_y);
    }

    let before_cursor = &text[..cursor_pos.min(text.len())];
    let mut col: usize = 0;
    let mut row: u16 = 0;

    for ch in before_cursor.chars() {
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            let ch_width = ch.width().unwrap_or(1);
            if col + ch_width > wrap_width {
                row += 1;
                col = ch_width;
            } else {
                col += ch_width;
            }
        }
    }

    (base_x + col as u16, base_y + row)
}

fn draw_tool_approval_popup(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(60, 40, area);

    let clear = Clear;
    f.render_widget(clear, popup_area);

    let block = Block::default()
        .title(" Tool Approval ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(focused_border_style())
        .style(bg_style());

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    if let Some(ref suggestion) = app.pending_suggestion {
        let content = vec![
            Line::from(vec![
                Span::styled("Model suggests using: ", text_style()),
                Span::styled(&suggestion.tool_name, tool_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Arguments: ", muted_style()),
                Span::styled(&suggestion.args, text_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Confidence: ", muted_style()),
                Span::styled(
                    format!("{:.0}%", suggestion.confidence * 100.0),
                    highlight_style(),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", text_style()),
                Span::styled("y", accent_style()),
                Span::styled(" to execute or ", text_style()),
                Span::styled("n", error_style()),
                Span::styled(" to skip", text_style()),
            ]),
        ];

        let paragraph = Paragraph::new(Text::from(content)).style(bg_style());
        f.render_widget(paragraph, inner);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Draw the multi-model comparison overlay (90% × 85% popup).
fn draw_comparison_overlay(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(90, 85, area);

    let clear = Clear;
    f.render_widget(clear, popup_area);

    let block = Block::default()
        .title(" 📊 Multi-Model Comparison ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(focused_border_style())
        .style(bg_style());

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Find the assistant message with the most secondary responses
    let target_msg = app.messages.iter()
        .filter(|m| m.role == "assistant")
        .max_by_key(|m| m.multi_model_responses.len());

    let mut lines: Vec<Line> = Vec::new();

    if let Some(msg) = target_msg {
        if msg.multi_model_responses.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("No alternate responses available yet.", muted_style()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Enable multi-model mode with /multi and send a message.", muted_style()),
            ]));
        } else {
            // Header — primary model
            lines.push(Line::from(vec![
                Span::styled("Primary: ", muted_style()),
                Span::styled(&app.model, highlight_style().add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(""));

            // Primary response (truncated to first 10 lines for header view)
            for line in msg.content.lines().take(10) {
                lines.push(Line::from(vec![Span::styled(line, text_style())]));
            }
            if msg.content.lines().count() > 10 {
                lines.push(Line::from(vec![Span::styled("...", muted_style())]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("─".repeat(inner.width as usize), border_style()),
            ]));
            lines.push(Line::from(""));

            // Secondary responses
            for (idx, sec) in msg.multi_model_responses.iter().enumerate() {
                let is_selected = idx == app.comparison_selected;
                let marker = if is_selected { "▶ " } else { "  " };
                let name_style = if is_selected {
                    highlight_style().add_modifier(Modifier::BOLD)
                } else {
                    accent_style()
                };

                lines.push(Line::from(vec![
                    Span::styled(marker, if is_selected { highlight_style() } else { muted_style() }),
                    Span::styled(&sec.model_name, name_style),
                    Span::styled("  |  ", muted_style()),
                    Span::styled(format!("{}ms", sec.latency_ms), text_style()),
                    Span::styled("  |  ", muted_style()),
                    Span::styled(format!("{} tokens", sec.tokens), text_style()),
                ]));

                // Show content for selected response, preview for others
                let preview_lines = if is_selected { 20 } else { 3 };
                for line in sec.content.lines().take(preview_lines) {
                    lines.push(Line::from(vec![
                        Span::styled("    ", muted_style()),
                        Span::styled(line, if is_selected { text_style() } else { muted_style() }),
                    ]));
                }
                if sec.content.lines().count() > preview_lines {
                    lines.push(Line::from(vec![
                        Span::styled("    ...", muted_style()),
                    ]));
                }
                lines.push(Line::from(""));
            }

            lines.push(Line::from(vec![
                Span::styled("↑/↓ to navigate • Ctrl+V to close", muted_style()),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("No assistant messages found.", muted_style()),
        ]));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: true })
        .style(bg_style());
    f.render_widget(paragraph, inner);
}

#[allow(dead_code)]
#[allow(dead_code)]
#[allow(dead_code)]
fn draw_chat_header(_f: &mut Frame, _area: Rect) {
    // Removed — welcome message is now in chat history
}
