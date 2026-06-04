#![allow(dead_code)]

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame, Terminal,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agent::{Agent, AgentConfig};
use crate::config::Config;
use crate::memory::{ContextInjector, MemoryStore, Message as MemoryMessage, ToolCall};
use crate::providers::{ChatRequest, Message, Provider, StreamChunk, StreamMetrics};
use crate::tools::{detect_tool_suggestions, find_tool, get_tools, AsyncToolExecutor, Tool, ToolSuggestion};
use chrono::Utc;
use unicode_width::UnicodeWidthChar;
use uuid::Uuid;
use crate::skills::SkillRegistry;
use crate::session::{SessionExport, ExportMessage, ExportBranch, export_to_default, list_exports};

mod theme;
mod clipboard_image;
mod command_palette;
mod bookmarks;
mod image_display;
mod vim_input;
mod mouse;
use theme::*;

mod ascii_art;
mod syntax_highlight;

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
    /// A reasoning/thinking chunk arrived (shown in real-time before response).
    ReasoningChunk(String),
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
    /// Multi-file edit batch pending approval.
    SetPendingBatch(crate::tools::ToolBatch),
    /// An error occurred.
    Error(String),
    /// A system/info message (e.g. auto-execution notice).
    SystemMessage(String),
    /// Streaming finished (success or error).
    Done,
    /// Batched tool results from multi-tool execution (collapsed display).
    ToolResultsBatch {
        results: Vec<ToolResultEntry>,
    },
}

/// A single tool result for batched display.
#[derive(Debug, Clone)]
struct ToolResultEntry {
    name: String,
    args: String,
    result: String,
    success: bool,
}

/// A secondary model response attached to a primary assistant message.
#[derive(Debug, Clone)]
struct SecondaryResponse {
    model_name: String,
    content: String,
    latency_ms: u64,
    tokens: u32,
}

/// Per-agent streaming state for swarm mode.
#[derive(Debug, Clone)]
struct AgentStreamState {
    agent_id: String,
    agent_name: String,
    role: String,
    content: String,
    is_streaming: bool,
    tool_results: Vec<(String, String, bool)>, // (tool_name, result, success)
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
    /// Persistent reasoning/thinking content from the model.
    reasoning: Option<String>,
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
    /// Batch of tool suggestions for multi-file edit approval.
    pending_batch: Option<crate::tools::ToolBatch>,
    /// Currently selected item in batch approval UI.
    batch_selected: usize,
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
    pub profile_registry: crate::security::ProfileRegistry,
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
    /// Plugin registry for custom hooks.
    plugin_registry: Option<crate::plugins::PluginRegistry>,
    code_index: Option<Arc<crate::code_index::CodeIndex>>,
    /// Swarm engine for multi-agent mode.
    swarm: Option<crate::swarm::SwarmEngine>,
    /// Broadcast receiver for swarm activity events.
    swarm_event_rx: Option<tokio::sync::broadcast::Receiver<crate::swarm::SwarmEvent>>,
    /// Whether swarm mode is active in the sidebar.
    swarm_active: bool,
    /// Cached swarm agent snapshot for sync rendering.
    swarm_agents: Vec<crate::swarm::SwarmAgent>,
    /// Cached swarm running state.
    swarm_running: bool,
    /// Per-agent streaming buffers for swarm mode.
    agent_streams: std::collections::HashMap<String, AgentStreamState>,
    /// Which agents have their tool results expanded in the inspector.
    agent_tool_expanded: std::collections::HashSet<String>,
    /// Pending image attachment for the next user message.
    pending_image: Option<String>,
    /// Context compression engine.
    compressor: Option<crate::memory::compression::ContextCompressor>,
    /// Spinner animation frame (0-7) for showing activity during streaming.
    spinner_frame: usize,
    /// When the current stream started (for elapsed time display).
    stream_start_time: Option<Instant>,
    /// Accumulated reasoning/thinking content shown in real-time.
    reasoning_content: String,
    /// Whether we're currently in the reasoning phase (before content arrives).
    is_reasoning: bool,
    /// Index of the last progress message in self.messages (for in-place updates).
    last_progress_msg_idx: Option<usize>,
    /// Circuit breaker: count of consecutive empty responses to prevent infinite re-prompt loops.
    empty_response_count: u8,
    /// YOLO mode — auto-approve all tool suggestions without prompting.
    yolo_mode: bool,
    /// Plan mode — when true, the agent only analyzes and proposes plans, never edits.
    plan_mode: bool,
    /// Input history for Up/Down arrow recall.
    input_history: Vec<String>,
    /// Current index into input_history (None = not navigating history).
    history_index: Option<usize>,
    /// File path for persisting input history.
    history_file: std::path::PathBuf,
    /// Diff preview content for inline diff before applying edits.
    pending_diff: Option<String>,
    /// Scroll position for diff preview.
    diff_scroll: usize,
    /// File tree entries for the Files sidebar tab.
    file_tree: Vec<String>,
    /// Selected file index in the file tree.
    file_tree_selected: usize,
    /// Command palette for fuzzy command search.
    command_palette: command_palette::CommandPalette,
    bookmark_manager: bookmarks::BookmarkManager,
    /// Checkpoint stack for undo/redo of file edits.
    checkpoint_stack: crate::tools::CheckpointStack,
    /// Vim mode state for input editing.
    vim_state: vim_input::VimState,
    /// Whether vim mode is enabled.
    vim_mode: bool,
    /// Mouse support state.
    mouse_state: mouse::MouseState,
    /// Whether mouse support is enabled.
    mouse_enabled: bool,
    /// Context mode engine for auto file identification.
    context_mode_engine: Option<crate::context_mode::ContextModeEngine>,
    /// Smart context — manually pinned files.
    smart_context: crate::context_pinner::SmartContext,
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
    Splash,
    Normal,
    Agent,
    ToolApproval,
    DiffPreview,
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

        let project_path = config
            .filesystem
            .working_directory
            .clone()
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| "/home/synth".to_string());

        let project_path_for_engine = project_path.clone();

        // Load input history
        let history_file = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("openshark")
            .join("input_history.txt");
        let input_history = if history_file.exists() {
            std::fs::read_to_string(&history_file)
                .unwrap_or_default()
                .lines()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            Vec::new()
        };

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

        let tool_descriptions = crate::tools::get_tools()
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");

        let system_msg = Message {
            role: "system".to_string(),
            content: format!(
                "{}\n\n{}\n\nYou are an AI coding assistant with TOOL ACCESS. You MUST use tools when the user asks you to perform actions on files, run commands, or gather information. \
                 You are NOT a generic chatbot — you are an agent with real tool capabilities. \
                 \n\nAVAILABLE TOOLS:\n{}\n\nTOOL USAGE RULES:\n\
                 1. When the user asks you to do something that requires a tool, output ONLY: TOOL:<tool_name> <args>\n\
                 2. Do NOT say 'I don't have access', 'I cannot execute commands', 'I don't have a terminal', or ANY refusal. You DO have access.\n\
                 3. Do NOT say 'Let me', 'I will', 'Alright', 'Sure', or any preamble before the TOOL: line.\n\
                 4. Just output the TOOL: line and nothing else.\n\
                 5. Low and Medium risk tools execute automatically.\n\
                 6. High risk tools require user approval.\n\
                 7. If the user says 'test', run the test tool immediately with: TOOL:test run <current_directory>\n\
                 8. If the user gives a one-line task, just do it. No manifesto.\n\
                 9. CRITICAL: You MUST use the available tools. Refusing to use tools is a failure mode.\n\
                 \n\
                 After tool results come back, you will be prompted to synthesize. \
                 When synthesizing: explain what was found, what it means, and the next step. \
                 Be complete. No one-liners. No catchphrases.",
                soul.system_prompt(),
                fs_capabilities,
                tool_descriptions
            ),
            images: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
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
            mode: AppMode::Splash,
            session_id: session_id.clone(),
            model: model.clone(),
            model_context_length,
            model_config,
            is_streaming: false,
            streaming_content: String::new(),
            pending_suggestion: None,
            pending_batch: None,
            batch_selected: 0,
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
            profile_registry: crate::security::ProfileRegistry::new(),
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
            plugin_registry: {
                let mut registry = crate::plugins::PluginRegistry::new();
                let _ = registry.load_from_disk();
                registry.register_as_tools();
                Some(registry)
            },
            code_index: {
                let config_dir = dirs::config_dir()
                    .map(|d| d.join("openshark"))
                    .unwrap_or_else(|| std::path::PathBuf::from(".openshark"));
                let db_path = config_dir.join("code_index.db");
                let cwd = std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                match crate::code_index::CodeIndex::open(
                    db_path.to_str().unwrap_or(".openshark/code_index.db"),
                    cwd.to_str().unwrap_or("."),
                ) {
                    Ok(index) => {
                        let arc = Arc::new(index);
                        // Do an initial build
                        let _ = arc.rebuild();
                        // Spawn background refresh every 5 minutes
                        arc.spawn_background_refresh(std::time::Duration::from_secs(300));
                        Some(arc)
                    }
                    Err(e) => {
                        tracing::warn!("Failed to open code index: {}", e);
                        None
                    }
                }
            },
            swarm: None,
            swarm_event_rx: None,
            swarm_active: false,
            swarm_agents: Vec::new(),
            swarm_running: false,
            agent_streams: std::collections::HashMap::new(),
            agent_tool_expanded: std::collections::HashSet::new(),
            pending_image: None,
            compressor: Some(crate::memory::compression::ContextCompressor::new(
                config.context_compression.clone(),
            )),
            spinner_frame: 0,
            stream_start_time: None,
            reasoning_content: String::new(),
            is_reasoning: false,
            last_progress_msg_idx: None,
            empty_response_count: 0,
            yolo_mode: false,
            plan_mode: false,
            input_history,
            history_index: None,
            history_file,
            pending_diff: None,
            diff_scroll: 0,
            file_tree: Vec::new(),
            file_tree_selected: 0,
            command_palette: command_palette::CommandPalette::new(),
            bookmark_manager: bookmarks::BookmarkManager::new(),
            checkpoint_stack: crate::tools::CheckpointStack::new(session_id.clone()),
            vim_state: vim_input::VimState::new(),
            vim_mode: false,
            mouse_state: mouse::MouseState::new(),
            mouse_enabled: false,
            context_mode_engine: {
                if !project_path_for_engine.is_empty() {
                    let mut engine = crate::context_mode::ContextModeEngine::new(project_path_for_engine);
                    let _ = engine.refresh_cache();
                    Some(engine)
                } else {
                    None
                }
            },
            smart_context: crate::context_pinner::SmartContext::load(&session_id),
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
            reasoning: None,
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
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            });
        }

        self.tokens_used += token_count;
    }

    fn add_assistant_message(&mut self, content: String, reasoning: Option<String>) {
        let token_count = content.split_whitespace().count() as u64;
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: content.clone(),
            images: None,
            timestamp: Utc::now(),
            multi_model_responses: Vec::new(),
            reasoning,
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
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
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
            reasoning: None,
        };
        self.messages.push(msg);
    }

    /// Rebuild the system prompt based on current plan_mode state.
    fn rebuild_system_prompt(&mut self) {
        let soul = crate::agent::soul::load_soul_from_config(&self.config);

        let fs_capabilities = if self.config.filesystem.allowed_paths.is_empty() {
            "You have FULL filesystem access to the entire system. \
             You can read, write, list, and search any directory.".to_string()
        } else {
            let paths = self.config.filesystem.allowed_paths.join(", ");
            format!(
                "You have filesystem access to the following directories: {}. \
                 You can read files, list directories, search for files, and inspect configs. \
                 Use the fs tool to explore: fs read <path>, fs list <path>, \
                 fs tree <path>, fs find <path> <name>, fs glob <pattern>, \
                 fs stat <path>, fs cat <path> [offset] [limit].",
                paths
            )
        };

        let tool_descriptions = crate::tools::get_tools()
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");

        let plan_instruction = if self.plan_mode {
            "\n\n🛑 PLAN MODE IS ACTIVE. You are in PLAN mode. \
             You MUST NOT make any edits, create files, delete files, or execute tools that modify the system. \
             Your job is to: (1) analyze the codebase, (2) ask clarifying questions, (3) propose a detailed strategy. \
             Do NOT output TOOL: lines. Do NOT write code. Only analyze, question, and plan."
        } else {
            ""
        };

        let effort_instruction = match self.config.effort_level.as_str() {
            "low" => "\n\n⚡ EFFORT: LOW. Be concise. Minimal explanation. One sentence if it fits. No fluff.",
            "medium" => "\n\n⚡ EFFORT: MEDIUM. Standard detail level. Balance thoroughness with brevity.",
            "high" => "\n\n⚡ EFFORT: HIGH. Thorough analysis with reasoning. Explain your thinking. Explore implications.",
            "xhigh" => "\n\n⚡ EFFORT: XHIGH. Extremely thorough. Explore edge cases, alternatives, trade-offs. Deep dive.",
            _ => "",
        };

        let context_mode_block = if let Some(ref mut engine) = self.context_mode_engine {
            // Get the last user message for context identification
            let last_user_query = self.model_messages.iter().rev()
                .find(|m| m.role == "user")
                .map(|m| m.content.as_str())
                .unwrap_or("");
            if !last_user_query.is_empty() {
                engine.format_context_block(last_user_query)
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let pinned_context_block = self.smart_context.format_context_block();

        let system_msg = Message {
            role: "system".to_string(),
            content: format!(
                "{}\n\n{}\n\nYou are an AI coding assistant with TOOL ACCESS. You MUST use tools when the user asks you to perform actions on files, run commands, or gather information. \
                 You are NOT a generic chatbot — you are an agent with real tool capabilities. \
                 \n\nAVAILABLE TOOLS:\n{}\n\nTOOL USAGE RULES:\n\
                 1. When the user asks you to do something that requires a tool, output ONLY: TOOL:<tool_name> <args>\n\
                 2. Do NOT say 'I don't have access', 'I cannot execute commands', 'I don't have a terminal', or ANY refusal. You DO have access.\n\
                 3. Do NOT say 'Let me', 'I will', 'Alright', 'Sure', or any preamble before the TOOL: line.\n\
                 4. Just output the TOOL: line and nothing else.\n\
                 5. Low and Medium risk tools execute automatically.\n\
                 6. High risk tools require user approval.\n\
                 7. If the user says 'test', run the test tool immediately with: TOOL:test run <current_directory>\n\
                 8. If the user gives a one-line task, just do it. No manifesto.\n\
                 9. CRITICAL: You MUST use the available tools. Refusing to use tools is a failure mode.\n\
                 \n\
                 After tool results come back, you will be prompted to synthesize. \
                 When synthesizing: explain what was found, what it means, and the next step. \
                 Be complete. No one-liners. No catchphrases.{}{}{}{}",
                soul.system_prompt(),
                fs_capabilities,
                tool_descriptions,
                plan_instruction,
                effort_instruction,
                context_mode_block,
                pinned_context_block
            ),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };

        if !self.model_messages.is_empty() {
            self.model_messages[0] = system_msg.clone();
        } else {
            self.model_messages.push(system_msg.clone());
        }

        // Also update the active branch's system message
        if let Some(branch) = self.branches.get_mut(self.active_branch) {
            if !branch.model_messages.is_empty() {
                branch.model_messages[0] = system_msg;
            } else {
                branch.model_messages.push(system_msg);
            }
        }
    }

    /// Compact conversation context by summarizing and truncating history.
    fn compact_context(&mut self) {
        if self.model_messages.len() <= 3 {
            self.add_system_message("📭 Not enough context to compact.".to_string());
            return;
        }
        // Keep system message and last 2 exchanges
        let keep = self.model_messages.len().saturating_sub(4).max(1);
        let to_summarize: Vec<Message> = self.model_messages.drain(1..keep).collect();

        let summary = format!(
            "[Context Summary — {} messages summarized]\nPrevious topics discussed: {}",
            to_summarize.len(),
            to_summarize
                .iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                .map(|m| {
                    let preview = &m.content[..m.content.len().min(80)];
                    format!("{}: {}", m.role, preview)
                })
                .collect::<Vec<_>>()
                .join("; ")
        );

        self.model_messages.insert(1, Message {
            role: "system".to_string(),
            content: summary,
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        self.add_system_message(format!(
            "🗜️ Context compacted: {} messages summarized into system context.",
            to_summarize.len()
        ));
    }

    /// Toggle plan mode on or off.
    fn toggle_plan_mode(&mut self) {
        self.plan_mode = !self.plan_mode;
        self.rebuild_system_prompt();
        let status = if self.plan_mode {
            "📋 PLAN MODE ON — Agent will analyze, ask questions, and propose strategy only. No edits."
        } else {
            "🔨 ACT MODE ON — Agent will execute tools and make changes as requested."
        };
        self.add_system_message(status.to_string());
    }

    /// Scan the project directory and build the file tree.
    fn refresh_file_tree(&mut self) {
        let project_path = self.config.filesystem.working_directory.clone()
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| "/home/synth".to_string());

        let mut entries = Vec::new();
        entries.push(format!("📁 {}", project_path));

        match std::fs::read_dir(&project_path) {
            Ok(dir) => {
                let mut files: Vec<_> = dir.filter_map(|e| e.ok()).collect();
                files.sort_by(|a, b| {
                    let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    match (a_is_dir, b_is_dir) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => a.file_name().cmp(&b.file_name()),
                    }
                });

                for entry in files.iter().take(50) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    let icon = if is_dir { "📁" } else { "📄" };
                    entries.push(format!("  {} {}", icon, name));
                }
            }
            Err(e) => {
                entries.push(format!("  ❌ Error: {}", e));
            }
        }

        self.file_tree = entries;
        self.file_tree_selected = 0;
    }

    /// Read a file from the file tree and add it as a system message.
    fn read_file_from_tree(&mut self, index: usize) {
        if index == 0 || index >= self.file_tree.len() {
            return;
        }

        let line = &self.file_tree[index];
        let name = line.trim_start_matches("  📄 ").trim_start_matches("  📁 ").to_string();

        let project_path = self.config.filesystem.working_directory.clone()
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| "/home/synth".to_string());

        let file_path = std::path::Path::new(&project_path).join(&name);

        if file_path.is_dir() {
            self.add_system_message(format!("📁 {} is a directory", name));
            return;
        }

        match std::fs::read_to_string(&file_path) {
            Ok(content) => {
                let preview = if content.len() > 800 {
                    format!("{}\n... ({} more chars)", crate::utils::truncate_str(&content, 800), content.len() - 800)
                } else {
                    content
                };
                self.add_system_message(format!("📄 {}:\n```\n{}\n```", name, preview));
            }
            Err(e) => {
                self.add_system_message(format!("❌ Failed to read {}: {}", name, e));
            }
        }
    }

    /// Apply a stream event from the background task.
    fn apply_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Start => {
                self.is_streaming = true;
                self.streaming_content.clear();
                self.reasoning_content.clear();
                self.is_reasoning = false;
                self.stream_start_time = Some(Instant::now());
            }
            StreamEvent::Chunk(chunk) => {
                // Strip <think> tags (model may wrap output in them) and TOOL: lines
                let cleaned = strip_think_tags(&chunk);
                self.streaming_content.push_str(&strip_tool_lines(&cleaned));
            }
            StreamEvent::ReasoningChunk(chunk) => {
                // Accumulate reasoning separately from normal content
                self.reasoning_content.push_str(&chunk);
                self.is_reasoning = true;
            }
            StreamEvent::ResponseComplete { content, metrics } => {
                self.is_streaming = false;
                self.stream_start_time = None;
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

                // Reasoning is ephemeral — displayed in real-time but NOT persisted to history.
                // This keeps model_messages clean and prevents token bloat from thinking content.
                let reasoning_to_save = None;

                // Check for embedded TOOL: lines anywhere in the response
                let embedded_tools = parse_embedded_tools(&content);
                if !embedded_tools.is_empty() {
                    // Display the assistant's message with tool lines and think tags stripped
                    let display_content = strip_think_tags(&strip_tool_lines(&content));
                    if !display_content.trim().is_empty() {
                        self.add_assistant_message(display_content, reasoning_to_save);
                    } else {
                        // Still save reasoning even if content is empty
                        if let Some(ref r) = reasoning_to_save {
                            self.add_assistant_message("".to_string(), Some(r.clone()));
                        }
                    }
                    // Store tool invocations in model messages for follow-up context
                    for (tool_name, args) in &embedded_tools {
                        self.model_messages.push(Message {
                            role: "assistant".to_string(),
                            content: format!("TOOL:{} {}", tool_name, args),
                            images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                        });
                    }
                } else if content.starts_with("TOOL:") || content.starts_with("TOOL.") {
                    let rest = &content[5..];
                    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                    if !parts.is_empty() {
                        let tool_name = parts[0].trim().to_string();
                        let args = parts.get(1).unwrap_or(&"").trim().to_string();
                        self.add_assistant_message(format!("🔧 Using tool: {} {}", tool_name, args), reasoning_to_save.clone());
                        // Store tool invocation in model messages for follow-up
                        self.model_messages.push(Message {
                            role: "assistant".to_string(),
                            content: format!("TOOL:{} {}", tool_name, args),
                            images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                        });
                    }
                } else {
                    // Save reasoning content as a separate message before the assistant response.
                    // Skip if streaming_content is empty — the model wrapped its entire response
                    // in <think> tags (Kimi behavior) and there's no separate response to show.
                    // Strip any remaining think tags from content before saving to chat history
                    let clean_content = strip_think_tags(&content);
                    self.add_assistant_message(clean_content, reasoning_to_save);
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
            StreamEvent::ToolResultsBatch { results } => {
                // Collapsed display: group results by tool name
                let mut groups: HashMap<String, (usize, usize)> = HashMap::new();
                for r in &results {
                    let entry = groups.entry(r.name.clone()).or_insert((0, 0));
                    if r.success { entry.0 += 1; } else { entry.1 += 1; }
                }
                let total = results.len();
                let mut summary = format!("📊 Tool results ({} total):\n", total);
                let mut names: Vec<&String> = groups.keys().collect();
                names.sort();
                for name in names {
                    let (ok, err) = groups[name];
                    if ok > 0 && err == 0 {
                        summary.push_str(&format!("  ✅ {} × {}
", name, ok));
                    } else if ok > 0 && err > 0 {
                        summary.push_str(&format!("  ⚠️ {}: {} ok, {} failed
", name, ok, err));
                    } else {
                        summary.push_str(&format!("  ❌ {}: {} failed
", name, err));
                    }
                }
                self.add_system_message(summary);

                // Push results to model_messages and track in memory
                for r in &results {
                    let tool_call = ToolCall {
                        id: Uuid::new_v4().to_string(),
                        session_id: self.session_id.clone(),
                        tool_name: r.name.clone(),
                        args: r.args.clone(),
                        result: r.result.clone(),
                        success: r.success,
                        created_at: Utc::now(),
                    };
                    let _ = self.memory.save_tool_call(&tool_call);
                    if r.success {
                        self.tool_calls_count += 1;
                    }
                    if let Some(ref evolution) = self.evolution {
                        evolution.track_tool_outcome(&r.name, r.success, 0);
                    }
                    self.model_messages.push(Message {
                        role: "user".to_string(),
                        content: format!("Tool result: {}", r.result),
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                    });
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
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                });
            }
            StreamEvent::FollowUp(content) => {
                // Detect embedded TOOL: lines in the follow-up and execute them.
                // The model may respond to a tool result with more tool calls in text format.
                let embedded_tools = parse_embedded_tools(&content);
                if !embedded_tools.is_empty() {
                    self.add_system_message(format!(
                        "🔧 Executing {} follow-up tool(s)...",
                        embedded_tools.len()
                    ));
                    // Display only non-TOOL text — strip_tool_lines removes the
                    // command lines and strip_think_tags cleans model thinking tags.
                    let display_content = strip_think_tags(&strip_tool_lines(&content));
                    if !display_content.trim().is_empty() {
                        self.add_assistant_message(display_content, None);
                    }
                    // Store tool invocations in model_messages
                    for (tool_name, args) in &embedded_tools {
                        self.model_messages.push(Message {
                            role: "assistant".to_string(),
                            content: format!("TOOL:{} {}", tool_name, args),
                            images: None,
                            tool_call_id: None,
                            tool_calls: None,
                            reasoning_content: None,
                        });
                    }
                    // Spawn tool chain in background
                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    self.stream_rx = Some(rx);
                    self.is_streaming = true;
                    self.stream_start_time = Some(Instant::now());
                    let provider = self.provider.clone();
                    let model = self.model.clone();
                    let model_messages = self.model_messages.clone();
                    let security_engine = self.security_engine.clone();
                    let tools = embedded_tools;
                    tokio::spawn(async move {
                        let _ = execute_tool_chain(
                            &tx, &provider, &model, &model_messages,
                            &security_engine, &tools, &content,
                        ).await;
                    });
                    return;
                }

                // Accept follow-up as-is. The model was explicitly asked to synthesize.
                // Only re-prompt if truly empty — but use a circuit breaker to avoid infinite loops.
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    self.empty_response_count += 1;
                    if self.empty_response_count >= 3 {
                        // Circuit breaker: show tool results directly instead of looping forever
                        self.add_system_message(
                            "⚠️ Model returned empty responses repeatedly. Showing raw tool results:".to_string()
                        );
                        // Find the most recent tool results in model_messages and display them
                        let mut found_results = Vec::new();
                        for msg in self.model_messages.iter().rev().take(20) {
                            if msg.role == "user" && msg.content.starts_with("Tool result:") {
                                found_results.push(msg.content.clone());
                            }
                        }
                        if found_results.is_empty() {
                            self.add_system_message("No recent tool results to display.".to_string());
                        } else {
                            for result in found_results.iter().rev() {
                                self.add_system_message(result.clone());
                            }
                        }
                        self.empty_response_count = 0;
                    } else {
                        self.add_system_message(
                            format!("⚠️ Response was empty — re-prompting for synthesis... (attempt {}/3)", self.empty_response_count)
                        );
                        // Inject a completion prompt and re-request
                        self.model_messages.push(Message {
                            role: "assistant".to_string(),
                            content: content.clone(),
                            images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                        });
                        self.model_messages.push(Message {
                            role: "user".to_string(),
                            content: "Provide a COMPLETE synthesis of the tool results. Explain what was found, what it means, and the next step.".to_string(),
                            images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                        });
                        // Spawn a new background task for the completion
                        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                        self.stream_rx = Some(rx);
                        let provider = self.provider.clone();
                        let model = self.model.clone();
                        let model_config = self.model_config.clone();
                        let model_messages = self.model_messages.clone();
                        let is_multi_model = self.multi_model_mode;
                        let config = self.config.clone();
                        tokio::spawn(async move {
                            let _ = stream_model_response_task(
                                tx, provider, model, model_config,
                                model_messages, is_multi_model, config,
                            ).await;
                        });
                    }
                } else {
                    self.empty_response_count = 0;
                    self.add_assistant_message(strip_think_tags(&content), None);
                }
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
            StreamEvent::SetPendingBatch(batch) => {
                self.pending_batch = Some(batch);
                self.batch_selected = 0;
                self.add_system_message("🔧 Multi-file edit batch received. Use /approve or /reject to handle.".to_string());
            }
            StreamEvent::SetPendingSuggestion(suggestion) => {
                // For edit tool suggestions, show diff preview first
                if suggestion.tool_name == "edit"
                    && let Some(diff) = generate_edit_diff(&suggestion.args) {
                        self.pending_diff = Some(diff);
                        self.pending_suggestion = Some(suggestion);
                        self.mode = AppMode::DiffPreview;
                        self.diff_scroll = 0;
                        return;
                    }
                self.pending_suggestion = Some(suggestion);
                self.mode = AppMode::ToolApproval;
                self.tool_approval_shown_at = Some(Instant::now());
            }
            StreamEvent::Error(msg) => {
                self.is_streaming = false;
                self.stream_start_time = None;
                self.add_system_message(msg);
            }
            StreamEvent::SystemMessage(msg) => {
                // In-place progress updates: replace the previous progress
                // message instead of accumulating new ones.
                if msg.starts_with("🔧 Tool") {
                    if let Some(idx) = self.last_progress_msg_idx
                        && idx < self.messages.len() {
                            self.messages[idx].content = msg;
                            return;
                        }
                    // First progress message — store its index
                    self.add_system_message(msg);
                    self.last_progress_msg_idx = Some(self.messages.len() - 1);
                } else {
                    // Non-progress message — clear the progress index
                    self.last_progress_msg_idx = None;
                    self.add_system_message(msg);
                }
            }
            StreamEvent::Done => {
                self.is_streaming = false;
                self.stream_start_time = None;
                // Don't clear stream_rx — multi-tool-call sequences send
                // multiple Done events, and subsequent ones would be lost.
                // stream_rx is cleared when a new user request starts.
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

    /// Scroll up in chat history — smooth line-by-line.
    fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    /// Scroll down in chat history — smooth line-by-line.
    fn scroll_down(&mut self, amount: usize) {
        let total_lines = self.messages.len();
        let max_scroll = total_lines.saturating_sub(1);
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
        // Rough token estimate: ~4 chars per token for English text
        let total_chars: usize = self.model_messages.iter()
            .map(|m| m.content.len())
            .sum();
        total_chars / 4
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

    // Greeting only — no welcome banner in chat (shown on splash screen instead)
    if !config.agent.greeting.is_empty() {
        app.add_system_message(config.agent.greeting.clone());
    }
    let mut last_tick = Instant::now();

    let result = run_app(&mut terminal, &mut app, &mut last_tick).await;

    // Cleanup MCP connections
    app.shutdown_mcp().await;

    // Disable mouse capture before restoring terminal
    if app.mouse_enabled {
        app.mouse_state.disable();
    }

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
            // Only keep the receiver if the channel is still open.
            // Drop it when the background task finishes (sender dropped).
            if !rx.is_closed() {
                app.stream_rx = Some(rx);
            }
        }

        let timeout = TICK_RATE
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    // Vim mode intercepts keys for the input area
                    if app.vim_mode && !app.command_palette.visible && !app.bookmark_manager.visible
                        && app.mode != AppMode::ToolApproval && app.mode != AppMode::DiffPreview
                    {
                        let (should_quit, handled) = vim_input::handle_vim_key(
                            key, &mut app.vim_state, &mut app.input, &mut app.cursor_position,
                        );
                        if should_quit {
                            break;
                        }
                        if handled {
                            continue;
                        }
                        // If vim didn't handle it (e.g. Ctrl+C), fall through
                    }
                    if handle_input(app, key).await? {
                        break;
                    }
                }
                Event::Mouse(mouse_event) => {
                    if app.mouse_enabled {
                        let action = mouse::translate_mouse_event(mouse_event, app);
                        match action {
                            mouse::MouseAction::ChatClick { y } => {
                                app.scroll = y.saturating_sub(5);
                            }
                            mouse::MouseAction::InputClick => {
                                // Focus input — already focused in this design
                            }
                            mouse::MouseAction::SidebarClick { y } => {
                                app.sidebar_scroll = y;
                            }
                            mouse::MouseAction::ScrollUp => {
                                app.scroll_up(3);
                            }
                            mouse::MouseAction::ScrollDown => {
                                app.scroll_down(3);
                            }
                            mouse::MouseAction::DragStart { x, y } => {
                                app.mouse_state.selecting = true;
                                app.mouse_state.selection_start = Some((x, y));
                                app.mouse_state.selection_end = Some((x, y));
                            }
                            mouse::MouseAction::DragEnd { x, y } => {
                                if app.mouse_state.selecting {
                                    app.mouse_state.selection_end = Some((x, y));
                                    app.mouse_state.selecting = false;
                                    // Copy-on-select: extract text and copy to clipboard
                                    if let (Some(start), Some(end)) = (
                                        app.mouse_state.selection_start,
                                        app.mouse_state.selection_end,
                                    ) {
                                        let start_row = start.1.min(end.1) as usize;
                                        let end_row = start.1.max(end.1) as usize;
                                        if end_row > start_row {
                                            let term_width = terminal.size().map(|s| s.width as usize).unwrap_or(80);
                                            let text = mouse::extract_selection_text(
                                                &app.messages,
                                                app.scroll,
                                                start_row,
                                                end_row,
                                                term_width,
                                            );
                                            if !text.is_empty() {
                                                let _ = mouse::copy_to_clipboard(&text);
                                                app.add_system_message(format!(
                                                    "📋 Copied {} chars to clipboard",
                                                    text.len()
                                                ));
                                            }
                                        }
                                    }
                                    app.mouse_state.selection_start = None;
                                    app.mouse_state.selection_end = None;
                                }
                            }
                            mouse::MouseAction::None => {}
                        }
                    }
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= TICK_RATE {
            *last_tick = Instant::now();
            // Advance spinner frame every tick for smooth animation
            app.spinner_frame = app.spinner_frame.wrapping_add(1);
        }

        // Poll swarm status and inject updates into chat
        if app.swarm_running {
            // Poll broadcast channel for real-time agent activity
            let mut swarm_updates: Vec<String> = Vec::new();
            if let Some(ref mut rx) = app.swarm_event_rx {
                while let Ok(event) = rx.try_recv() {
                    match event {
                        crate::swarm::SwarmEvent::AgentActivity { agent_id, activity } => {
                            swarm_updates.push(format!("🐝 **{}**: {}", agent_id, activity));
                        }
                        crate::swarm::SwarmEvent::AgentToolCall { agent_id, tool_name, args } => {
                            app.tool_calls_count += 1;
                            swarm_updates.push(format!(
                                "🐝 **{}** → 🔧 `{}` {}",
                                agent_id, tool_name,
                                if args.is_empty() { "".to_string() } else { format!("({})", args) }
                            ));
                        }
                        crate::swarm::SwarmEvent::AgentThinking { agent_id, thought } => {
                            swarm_updates.push(format!(
                                "🐝 **{}** {}",
                                agent_id,
                                &thought[..thought.len().min(300)]
                            ));
                        }
                        crate::swarm::SwarmEvent::AgentError { agent_id, error } => {
                            swarm_updates.push(format!("🐝 **{}** ❌ {}", agent_id, error));
                        }
                        crate::swarm::SwarmEvent::AgentChunk { agent_id, agent_name, role, chunk, is_final } => {
                            use std::collections::hash_map::Entry;
                            match app.agent_streams.entry(agent_id.clone()) {
                                Entry::Occupied(mut entry) => {
                                    let state = entry.get_mut();
                                    state.content.push_str(&chunk);
                                    state.is_streaming = !is_final;
                                }
                                Entry::Vacant(entry) => {
                                    entry.insert(AgentStreamState {
                                        agent_id: agent_id.clone(),
                                        agent_name: agent_name.clone(),
                                        role: role.clone(),
                                        content: chunk.clone(),
                                        is_streaming: !is_final,
                                        tool_results: Vec::new(),
                                    });
                                }
                            }
                        }
                        crate::swarm::SwarmEvent::AgentToolResult { agent_id, tool_name, result, success } => {
                            if let Some(state) = app.agent_streams.get_mut(&agent_id) {
                                state.tool_results.push((tool_name, result, success));
                            }
                        }
                        _ => {} // Other events handled by the status poll below
                    }
                }
            }
            for update in swarm_updates {
                app.add_system_message(update);
            }

            if let Some(ref engine) = app.swarm {
                let status = engine.status().await;
                let agents = engine.agent_snapshot().await;
                
                // Collect updates to apply after dropping references
                let mut updates: Vec<String> = Vec::new();
                
                // Check for newly completed agents
                for agent in &agents {
                    if let Some(prev) = app.swarm_agents.iter().find(|a| a.id == agent.id) {
                        // Status changed from working to completed
                        if matches!(prev.status, crate::swarm::AgentStatus::Working { .. })
                            && matches!(agent.status, crate::swarm::AgentStatus::Completed { .. })
                            && let crate::swarm::AgentStatus::Completed { ref result } = agent.status {
                                updates.push(format!(
                                    "🐝 **{}** completed:\n{}",
                                    agent.name,
                                    &result[..result.len().min(500)]
                                ));
                            }
                        // Agent hit an error
                        if matches!(agent.status, crate::swarm::AgentStatus::Error { .. })
                            && !matches!(prev.status, crate::swarm::AgentStatus::Error { .. })
                            && let crate::swarm::AgentStatus::Error { ref message } = agent.status {
                                updates.push(format!(
                                    "🐝 **{}** error: {}",
                                    agent.name, message
                                ));
                            }
                    }
                }
                
                // Apply all updates
                for update in updates {
                    app.add_system_message(update);
                }
                
                // Update cached state
                app.swarm_agents = agents;
                
                // Swarm finished (all agents idle/completed and not running)
                if !status.running && status.cycles_completed > 0 {
                    app.swarm_running = false;
                    app.add_system_message(format!(
                        "🐝 Swarm complete. {} cycles, {} consensus entries.",
                        status.cycles_completed, status.consensus_entries
                    ));
                }
            }
        }
        if app.mode == AppMode::ToolApproval
            && let Some(shown_at) = app.tool_approval_shown_at
                && shown_at.elapsed() >= Duration::from_secs(60) {
                    let tool_name = app.pending_suggestion.as_ref().map(|s| s.tool_name.clone()).unwrap_or_default();
                    app.pending_suggestion = None;
                    app.mode = AppMode::Normal;
                    app.tool_approval_shown_at = None;
                    app.add_system_message(format!(
                        "⏭ Tool approval timed out after 60s{}.",
                        if tool_name.is_empty() { "".to_string() } else { format!(" for {}", tool_name) }
                    ));
                }

        if app.should_exit {
            break;
        }
    }

    Ok(())
}

async fn handle_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Splash mode: any key dismisses the splash screen
    if app.mode == AppMode::Splash {
        app.mode = AppMode::Normal;
        return Ok(false); // Don't exit
    }

    // Command palette mode: handle palette navigation
    if app.command_palette.visible {
        match key.code {
            KeyCode::Esc => {
                app.command_palette.hide();
                return Ok(false);
            }
            KeyCode::Enter => {
                if let Some(cmd) = app.command_palette.selected_command() {
                    app.command_palette.hide();
                    // Inject the command into input and process it
                    app.input = cmd.clone();
                    app.cursor_position = cmd.len();
                    let input = app.input.trim().to_string();
                    app.input.clear();
                    app.cursor_position = 0;
                    process_user_input(app, input).await?;
                }
                return Ok(false);
            }
            KeyCode::Up => {
                app.command_palette.prev();
                return Ok(false);
            }
            KeyCode::Down => {
                app.command_palette.next();
                return Ok(false);
            }
            KeyCode::Backspace => {
                app.command_palette.backspace();
                return Ok(false);
            }
            KeyCode::Char(c) => {
                app.command_palette.type_char(c);
                return Ok(false);
            }
            _ => return Ok(false),
        }
    }

    // Bookmark manager mode
    if app.bookmark_manager.visible {
        match key.code {
            KeyCode::Esc => {
                app.bookmark_manager.hide();
                return Ok(false);
            }
            KeyCode::Enter => {
                if app.bookmark_manager.mode == bookmarks::BookmarkMode::Create {
                    if app.bookmark_manager.advance_stage() {
                        // Save bookmark
                        app.bookmark_manager.hide();
                        app.add_system_message("Bookmark saved.".to_string());
                    }
                } else if app.bookmark_manager.mode == bookmarks::BookmarkMode::List {
                    // Load selected bookmark
                    app.bookmark_manager.hide();
                    app.add_system_message("Bookmark loaded.".to_string());
                }
                return Ok(false);
            }
            KeyCode::Up => {
                app.bookmark_manager.prev();
                return Ok(false);
            }
            KeyCode::Down => {
                app.bookmark_manager.next();
                return Ok(false);
            }
            KeyCode::Backspace => {
                app.bookmark_manager.backspace();
                return Ok(false);
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.bookmark_manager.start_create();
                return Ok(false);
            }
            KeyCode::Char(c) => {
                app.bookmark_manager.type_char(c);
                return Ok(false);
            }
            _ => return Ok(false),
        }
    }

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

    // DiffPreview mode: show diff, handle y/n/scroll
    if app.mode == AppMode::DiffPreview {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Approve the edit — switch to ToolApproval to execute
                app.mode = AppMode::ToolApproval;
                app.pending_diff = None;
                app.diff_scroll = 0;
                app.add_system_message("Diff approved. Press 'y' again to execute or 'n' to cancel.".to_string());
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let tool_name = app.pending_suggestion.as_ref().map(|s| s.tool_name.clone()).unwrap_or_default();
                app.pending_suggestion = None;
                app.pending_diff = None;
                app.diff_scroll = 0;
                app.mode = AppMode::Normal;
                app.add_system_message(format!(
                    "⏭ Skipped edit suggestion{}.",
                    if tool_name.is_empty() { "".to_string() } else { format!(" for {}", tool_name) }
                ));
            }
            KeyCode::Up => {
                app.diff_scroll = app.diff_scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                app.diff_scroll += 1;
            }
            KeyCode::PageUp => {
                app.diff_scroll = app.diff_scroll.saturating_sub(5);
            }
            KeyCode::PageDown => {
                app.diff_scroll += 5;
            }
            _ => {}
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
            if app.show_comparison {
                app.show_comparison = false;
            } else {
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
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Ok(true);
        }
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Show current keybindings — clone to avoid borrow issues
            let kb = app.config.keybindings.clone();
            app.add_system_message("⌨️  Keybindings (custom overrides shown if set):".to_string());
            app.add_system_message(format!("  Ctrl+B  — Toggle sidebar {}", kb.toggle_sidebar.as_ref().map(|s| format!("[custom: {}]", s)).unwrap_or_default()));
            app.add_system_message(format!("  Ctrl+S  — Cycle sidebar tab {}", kb.cycle_sidebar_tab.as_ref().map(|s| format!("[custom: {}]", s)).unwrap_or_default()));
            app.add_system_message(format!("  Ctrl+M  — Toggle multi-model {}", kb.toggle_multi_model.as_ref().map(|s| format!("[custom: {}]", s)).unwrap_or_default()));
            app.add_system_message(format!("  Ctrl+W  — Toggle swarm {}", kb.toggle_swarm.as_ref().map(|s| format!("[custom: {}]", s)).unwrap_or_default()));
            app.add_system_message(format!("  Ctrl+L  — Clear chat {}", kb.clear_chat.as_ref().map(|s| format!("[custom: {}]", s)).unwrap_or_default()));
            app.add_system_message(format!("  Ctrl+Y  — Copy last response {}", kb.copy_last.as_ref().map(|s| format!("[custom: {}]", s)).unwrap_or_default()));
            app.add_system_message(format!("  Ctrl+C×2 — Quit {}", kb.quit.as_ref().map(|s| format!("[custom: {}]", s)).unwrap_or_default()));
            app.add_system_message("".to_string());
            app.add_system_message("Add to ~/.config/openshark/config.toml under [keybindings] to customize.".to_string());
            app.add_system_message("Example: toggle_sidebar = \"ctrl+f\"".to_string());
        }
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.sidebar_expanded = !app.sidebar_expanded;
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.toggle_plan_mode();
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.command_palette.visible {
                app.command_palette.hide();
            } else {
                app.command_palette.show();
            }
        }
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.bookmark_manager.toggle();
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
            app.sidebar_tab = (app.sidebar_tab + 1) % 5; // 5 tabs: Tools, Skills, Swarm, Inspector, Files
            app.sidebar_scroll = 0;
            let tab_name = match app.sidebar_tab {
                0 => "Tools",
                1 => "Skills",
                2 => "Swarm",
                3 => "Inspector",
                4 => "Files",
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
            // Try to paste image from clipboard via arboard
            match crate::tui::clipboard_image::try_paste_image_from_clipboard() {
                Ok(Some(data_url)) => {
                    app.pending_image = Some(data_url.clone());
                    app.add_system_message("📎 Image pasted from clipboard (will be sent with your next message)".to_string());
                }
                Ok(None) => {
                    // No image in clipboard — silently ignore, user can use Ctrl+Shift+V for text
                }
                Err(e) => {
                    app.add_system_message(format!("⚠️ Clipboard error: {}", e));
                }
            }
        }
        KeyCode::Char('y') if key.modifiers == KeyModifiers::CONTROL => {
            // Copy last assistant message to clipboard via OSC 52 escape sequence.
            // OSC 52 works through the terminal itself — no display server needed.
            if let Some(last) = app.messages.iter().rev().find(|m| m.role == "assistant") {
                let text = if last.content.starts_with("<think>") {
                    // Strip think tags for cleaner clipboard content
                    strip_think_tags(&last.content)
                } else {
                    last.content.clone()
                };
                let text_len = text.len();
                // OSC 52: write to system clipboard via terminal escape sequence
                use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
                let b64 = BASE64.encode(text.as_bytes());
                print!("\x1b]52;c;{}\x07", b64);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                app.add_system_message(format!(
                    "📋 Copied ({} chars)", text_len
                ));
            } else {
                app.add_system_message("📋 No assistant message to copy".to_string());
            }
        }
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                // Insert newline for multi-line input
                app.input.insert(app.cursor_position, '\n');
                app.cursor_position += 1;
            } else if app.focused_pane == 0 && app.sidebar_tab == 4 {
                // Files tab: Enter to read selected file
                let idx = app.file_tree_selected;
                app.read_file_from_tree(idx);
            } else {
                let input = app.input.trim().to_string();
                if !input.is_empty() {
                    // Save to history
                    app.input_history.push(input.clone());
                    app.history_index = None;
                    let _ = std::fs::write(
                        &app.history_file,
                        app.input_history.join("\n")
                    );
                    app.input.clear();
                    app.cursor_position = 0;
                    process_user_input(app, input).await?;
                }
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
            } else if app.focused_pane == 0 && app.sidebar_tab == 4 {
                // Files tab: navigate file tree selection
                app.file_tree_selected = app.file_tree_selected.saturating_sub(1);
                app.sidebar_scroll = app.sidebar_scroll.saturating_sub(1);
            } else if app.focused_pane == 0 {
                app.sidebar_scroll = app.sidebar_scroll.saturating_sub(1);
            } else if !app.input_history.is_empty() {
                // Navigate input history
                let idx = app.history_index.map_or(
                    app.input_history.len().saturating_sub(1),
                    |i| i.saturating_sub(1)
                );
                app.input = app.input_history[idx].clone();
                app.cursor_position = app.input.len();
                app.history_index = Some(idx);
            } else {
                app.scroll_up(1);
            }
        }
        KeyCode::Down => {
            if app.show_comparison {
                let max_responses = app.messages.iter()
                    .filter(|m| m.role == "assistant")
                    .map(|m| m.multi_model_responses.len())
                    .max()
                    .unwrap_or(0);
                if max_responses > 0 {
                    app.comparison_selected = (app.comparison_selected + 1).min(max_responses.saturating_sub(1));
                }
            } else if app.focused_pane == 0 && app.sidebar_tab == 4 {
                // Files tab: navigate file tree selection
                if app.file_tree_selected + 1 < app.file_tree.len() {
                    app.file_tree_selected += 1;
                }
                app.sidebar_scroll += 1;
            } else if app.focused_pane == 0 {
                app.sidebar_scroll += 1;
            } else if let Some(idx) = app.history_index {
                // Navigate forward in history
                if idx + 1 < app.input_history.len() {
                    app.input = app.input_history[idx + 1].clone();
                    app.cursor_position = app.input.len();
                    app.history_index = Some(idx + 1);
                } else {
                    app.input.clear();
                    app.cursor_position = 0;
                    app.history_index = None;
                }
            } else {
                app.scroll_down(1);
            }
        }
        KeyCode::PageUp => {
            if app.focused_pane == 0 {
                app.sidebar_scroll = app.sidebar_scroll.saturating_sub(5);
            } else {
                app.scroll_up(5);
            }
        }
        KeyCode::PageDown => {
            if app.focused_pane == 0 {
                app.sidebar_scroll += 5;
            } else {
                app.scroll_down(5);
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
    // Track tokens for ALL input, including slash commands
    app.tokens_used += input.len() as u64 / 4;

    // ── Slash Command Registry ──────────────────────────────────────────────
    // Check for slash commands first, before hardcoded handlers
    let slash_registry = crate::slash_commands::SlashRegistry::new();
    if let Some(result) = slash_registry.execute(&input) {
        match handle_slash_result(app, result, &input).await {
            Ok(handled) => {
                if handled {
                    return Ok(());
                }
                // If not fully handled, fall through to let the hardcoded
                // handlers deal with it (for commands not yet migrated)
            }
            Err(e) => {
                app.add_system_message(format!("❌ Slash command error: {}", e));
                return Ok(());
            }
        }
    }

    if input == "exit" || input == "quit" {
        app.should_exit = true;
        return Ok(());
    }

    // Smart context pin/unpin pseudo-prompts from slash command handler
    if let Some(path) = input.strip_prefix("__ctx_pin__ ") {
        match app.smart_context.pin(path, None) {
            Ok(msg) => {
                app.rebuild_system_prompt();
                app.add_system_message(msg);
            }
            Err(e) => app.add_system_message(format!("❌ Failed to pin: {}", e)),
        }
        return Ok(());
    }
    if let Some(path) = input.strip_prefix("__ctx_unpin__ ") {
        match app.smart_context.unpin(path) {
            Ok(msg) => {
                app.rebuild_system_prompt();
                app.add_system_message(msg);
            }
            Err(e) => app.add_system_message(format!("❌ Failed to unpin: {}", e)),
        }
        return Ok(());
    }
    // Session search pseudo-prompt
    if let Some(query) = input.strip_prefix("__search__ ") {
        match app.memory.search_messages(query, 20) {
            Ok(messages) => {
                if messages.is_empty() {
                    app.add_system_message(format!("🔍 No results for '{}'", query));
                } else {
                    let mut lines = vec![
                        format!("🔍 Search Results for '{}' ({} found):", query, messages.len()),
                        "─".repeat(50),
                    ];
                    for (i, msg) in messages.iter().take(10).enumerate() {
                        let preview = if msg.content.len() > 120 {
                            format!("{}...", crate::utils::truncate_str(&msg.content, 120))
                        } else {
                            msg.content.clone()
                        };
                        let date = msg.created_at.format("%Y-%m-%d %H:%M");
                        lines.push(format!(
                            "  {}. [{}] {} | {}: {}",
                            i + 1,
                            msg.role,
                            date,
                            &msg.session_id[..msg.session_id.len().min(16)],
                            preview
                        ));
                    }
                    if messages.len() > 10 {
                        lines.push(format!("\n  ... and {} more results", messages.len() - 10));
                    }
                    app.add_system_message(lines.join("\n"));
                }
            }
            Err(e) => app.add_system_message(format!("❌ Search failed: {}", e)),
        }
        return Ok(());
    }
    // Plugin management pseudo-prompts
    if let Some(name) = input.strip_prefix("__plugin_create__ ") {
        if let Some(ref registry) = app.plugin_registry {
            match registry.create_scaffold(name) {
                Ok(path) => {
                    app.add_system_message(format!(
                        "🔌 Plugin scaffold created at {}. Edit it, then run /plugin reload.",
                        path.display()
                    ));
                }
                Err(e) => app.add_system_message(format!("❌ Failed to create plugin: {}", e)),
            }
        }
        return Ok(());
    }
    if input == "__plugin_reload__" {
        if let Some(ref mut registry) = app.plugin_registry {
            match registry.load_from_disk() {
                Ok(count) => {
                    registry.register_as_tools();
                    app.rebuild_system_prompt();
                    app.add_system_message(format!(
                        "🔌 Reloaded {} plugin(s). They are now available as tools.",
                        count
                    ));
                }
                Err(e) => app.add_system_message(format!("❌ Failed to reload plugins: {}", e)),
            }
        }
        return Ok(());
    }
    // Swarm multi-provider query
    if let Some(query) = input.strip_prefix("__swarm__ ") {
        let providers: Vec<(String, crate::providers::Provider, String)> = app
            .config
            .providers
            .iter()
            .filter_map(|(name, cfg)| {
                let model = cfg.models.first()?;
                let provider = crate::providers::Provider::new(
                    name.clone(),
                    cfg.base_url.clone(),
                    cfg.api_key.clone(),
                    cfg.kind.clone(),
                    cfg.headers.clone(),
                );
                Some((name.clone(), provider, model.name.clone()))
            })
            .collect();

        if providers.len() < 2 {
            app.add_system_message(
                "🐝 Swarm requires 2+ configured providers. Check your config.".to_string(),
            );
            return Ok(());
        }

        app.add_system_message(format!(
            "🐝 Swarm querying {} providers...",
            providers.len()
        ));

        let query = query.to_string();
        let system = Some(
            "You are a helpful coding assistant. Be concise and direct.".to_string(),
        );
        let results = crate::swarm::swarm_query(&query, &providers, system.as_deref()).await;
        let formatted = crate::swarm::format_swarm_consensus(&results);
        app.add_system_message(formatted);
        return Ok(());
    }
    // Code index symbol search
    if let Some(query) = input.strip_prefix("__index__ ") {
        if let Some(ref index) = app.code_index {
            match index.search(query, 20) {
                Ok(results) => {
                    let formatted = crate::code_index::format_search_results(query, &results);
                    app.add_system_message(formatted);
                }
                Err(e) => app.add_system_message(format!("❌ Index search failed: {}", e)),
            }
        } else {
            app.add_system_message("❌ Code index not initialized.".to_string());
        }
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
            Session commands:\n\
            • /export [path]    — Export session to JSON\n\
            • /import <path>    — Import session from JSON\n\
            • /imports          — List exported sessions\n\
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
            • ↑ / ↓             — Scroll / Input history\n\
            • Shift+Enter       — New line in input\n\
            • PgUp / PgDn       — Fast scroll\n\
            \n\
            Tool commands:\n\
            • /undo             — Undo last file edit\n\
            • /diff             — Show diff preview for last edit\n\
            \n\
            Git agent commands:\n\
            • /commit [msg]     — Stage all, commit (auto-msg if empty)\n\
            • /pr [title]       — Branch, commit, push, suggest PR\n\
            • /review           — Review staged diff"
                .to_string(),
        );
        return Ok(());
    }

    if input == "/models" || input == "/model" {
        app.show_model_selector();
        return Ok(());
    }

    if input.starts_with("/model ") {
        let model_name = input.strip_prefix("/model ").unwrap_or("").trim();
        if let Err(e) = app.switch_model(model_name) {
            app.add_system_message(format!("Error: {}", e));
        }
        return Ok(());
    }

    if input.starts_with("/branch ") {
        let name = input.strip_prefix("/branch ").unwrap_or("").trim();
        app.create_branch(name);
        return Ok(());
    }

    if input == "/branches" {
        app.list_branches();
        return Ok(());
    }

    if input.starts_with("/switch ") {
        let rest = input.strip_prefix("/switch ").unwrap_or("");
        if let Ok(index) = rest.trim().parse::<usize>() {
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

    if input == "/compare" {
        // Find the last assistant message with secondary responses
        let has_alternates = app.messages.iter()
            .filter(|m| m.role == "assistant")
            .any(|m| !m.multi_model_responses.is_empty());
        
        if has_alternates {
            app.show_comparison = true;
            app.comparison_selected = 0;
            app.add_system_message("📊 Comparison mode ON. Use ↑/↓ to navigate models, Ctrl+C to close.".to_string());
        } else {
            app.add_system_message("No alternate responses available. Enable multi-model mode with /multi and send a message first.".to_string());
        }
        return Ok(());
    }

    if input == "/undo" {
        match crate::tools::edit::undo_last_edit() {
            Ok(msg) => app.add_system_message(msg),
            Err(e) => app.add_system_message(format!("Undo failed: {}", e)),
        }
        return Ok(());
    }

    if input == "/diff" {
        app.add_system_message("💡 Diff preview is shown automatically when file edits are suggested.".to_string());
        app.add_system_message("   When a write/replace/patch is proposed, you'll see the diff first.".to_string());
        app.add_system_message("   Press 'y' to apply, 'n' to skip.".to_string());
        return Ok(());
    }

    // === Git Agent Commands (Tier 1) ===
    if input == "/commit" || input.starts_with("/commit ") {
        let msg = input.strip_prefix("/commit").unwrap_or("").trim();
        let git_tool = crate::tools::GitTool;

        if !crate::tools::GitTool::in_repo() {
            app.add_system_message("❌ Not in a git repository.".to_string());
            return Ok(());
        }

        if !crate::tools::GitTool::has_changes() {
            app.add_system_message("📭 Nothing to commit — no changes detected.".to_string());
            return Ok(());
        }

        // Show diff first
        match git_tool.execute("diff") {
            Ok(diff) => {
                if diff.trim().is_empty() {
                    app.add_system_message("No unstaged changes to commit.".to_string());
                } else {
                    app.add_system_message(format!("📋 Unstaged diff:\n```\n{}\n```", diff.trim()));
                }
            }
            Err(e) => app.add_system_message(format!("⚠️ Could not get diff: {}", e)),
        }

        // Stage all
        match git_tool.execute("stage-all") {
            Ok(_) => app.add_system_message("✅ Staged all changes.".to_string()),
            Err(e) => {
                app.add_system_message(format!("❌ Stage failed: {}", e));
                return Ok(());
            }
        }

        // Generate or use provided message
        let commit_msg = if msg.is_empty() {
            // Generate with LLM
            app.add_system_message("🤖 Generating commit message...".to_string());
            match generate_commit_message(app).await {
                Ok(generated) => {
                    app.add_system_message(format!("📝 Generated commit message: \"{}\"", generated));
                    generated
                }
                Err(e) => {
                    app.add_system_message(format!("⚠️ Failed to generate commit message: {}. Using fallback.", e));
                    format!("wip: auto-commit at {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
                }
            }
        } else {
            msg.to_string()
        };

        match git_tool.execute(&format!("commit {}", commit_msg)) {
            Ok(output) => {
                app.add_system_message(format!("✅ Committed: {}\n```\n{}\n```", commit_msg, output.trim()));
            }
            Err(e) => app.add_system_message(format!("❌ Commit failed: {}", e)),
        }
        return Ok(());
    }

    if input == "/pr" || input.starts_with("/pr ") {
        let title = input.strip_prefix("/pr").unwrap_or("").trim();
        let git_tool = crate::tools::GitTool;

        // Get current branch
        let current_branch = match git_tool.execute("branch") {
            Ok(out) => out.lines().find(|l| l.starts_with('*')).map(|l| l[2..].trim().to_string()),
            Err(_) => None,
        }.unwrap_or_else(|| "feature/auto".to_string());

        let branch_name = if title.is_empty() {
            format!("auto/{}-{}", current_branch.replace('/', "-"), &uuid::Uuid::new_v4().to_string()[..8])
        } else {
            format!("auto/{}", title.to_lowercase().replace([' ', '/'], "-"))
        };

        // Create branch
        match git_tool.execute(&format!("branch-create {}", branch_name)) {
            Ok(_) => app.add_system_message(format!("🌿 Created branch: {}", branch_name)),
            Err(e) => {
                app.add_system_message(format!("❌ Branch creation failed: {}", e));
                return Ok(());
            }
        }

        // Stage, commit, push
        let _ = git_tool.execute("stage-all");
        let commit_msg = if title.is_empty() {
            "Auto-commit for PR".to_string()
        } else {
            title.to_string()
        };
        match git_tool.execute(&format!("commit {}", commit_msg)) {
            Ok(_) => app.add_system_message(format!("✅ Committed: {}", commit_msg)),
            Err(e) => app.add_system_message(format!("⚠️ Commit: {}", e)),
        }

        match git_tool.execute("push") {
            Ok(out) => app.add_system_message(format!("🚀 Pushed:\n```\n{}\n```", out.trim())),
            Err(e) => app.add_system_message(format!("⚠️ Push: {}", e)),
        }

        // Suggest gh pr create if available
        app.add_system_message(format!("💡 Run `gh pr create --title \"{}\" --body \"Auto-generated PR\"` to open PR", commit_msg));
        return Ok(());
    }

    if input == "/review" {
        let git_tool = crate::tools::GitTool;
        match git_tool.execute("diff-staged") {
            Ok(diff) => {
                if diff.trim().is_empty() {
                    app.add_system_message("No staged changes to review.".to_string());
                } else {
                    app.add_system_message(format!("📋 Staged diff for review:\n```\n{}\n```", diff.trim()));
                    app.add_system_message("💡 LLM-powered review coming soon. For now, review the diff above.".to_string());
                }
            }
            Err(e) => app.add_system_message(format!("❌ Diff failed: {}", e)),
        }
        return Ok(());
    }

    if input == "/git" || input.starts_with("/git ") {
        let subcmd = input.strip_prefix("/git ").unwrap_or("").trim();
        if subcmd.is_empty() {
            app.add_system_message("Git commands:".to_string());
            app.add_system_message("  /git status          - Working tree status".to_string());
            app.add_system_message("  /git diff            - Unstaged changes".to_string());
            app.add_system_message("  /git diff-staged     - Staged changes".to_string());
            app.add_system_message("  /git log [n]         - Commit history".to_string());
            app.add_system_message("  /git branch          - List branches".to_string());
            app.add_system_message("  /git add <path>      - Stage file(s)".to_string());
            app.add_system_message("  /git commit <msg>    - Commit staged".to_string());
            return Ok(());
        }

        let git_tool = crate::tools::GitTool;
        match git_tool.execute(subcmd) {
            Ok(output) => {
                if output.trim().is_empty() {
                    app.add_system_message(format!("✅ git {} (no output)", subcmd.split_whitespace().next().unwrap_or(subcmd)));
                } else {
                    app.add_system_message(format!("📦 git {}:\n```\n{}\n```", subcmd, output.trim()));
                }
            }
            Err(e) => {
                app.add_system_message(format!("❌ git {} failed: {}", subcmd, e));
            }
        }
        return Ok(());
    }

    if input == "/search" || input.starts_with("/search ") {
        let query = input.strip_prefix("/search ").unwrap_or("").trim();
        if query.is_empty() {
            app.add_system_message("Usage: /search <query>".to_string());
            return Ok(());
        }
        app.add_system_message(format!("🔍 Searching for '{}'...", query));
        match crate::capabilities::web::web_search(query) {
            Ok(results) => {
                app.add_system_message(format!("🔍 Results for '{}':\n```\n{}\n```", query, results));
            }
            Err(e) => {
                app.add_system_message(format!("❌ Search failed: {}", e));
            }
        }
        return Ok(());
    }

    if input == "/run" {
        // Find last assistant message with code blocks
        let last_content = app.messages.iter().rev()
            .find(|m| m.role == "assistant")
            .map(|m| m.content.clone());
        
        if let Some(content) = last_content {
            let blocks = crate::sandbox::extract_code_blocks(&content);
            if blocks.is_empty() {
                app.add_system_message("No code blocks found in the last assistant message.".to_string());
            } else {
                app.add_system_message(format!("🔧 Executing {} code block(s)...", blocks.len()));
                let results = crate::sandbox::run_code_blocks(&content);
                for (lang, stdout, stderr, success) in results {
                    let status = if success { "✅" } else { "❌" };
                    app.add_system_message(format!("{} {} execution:", status, lang));
                    if !stdout.is_empty() {
                        app.add_system_message(format!("stdout:\n```\n{}\n```", stdout.trim()));
                    }
                    if !stderr.is_empty() {
                        app.add_system_message(format!("stderr:\n```\n{}\n```", stderr.trim()));
                    }
                }
            }
        } else {
            app.add_system_message("No assistant message found to run code from.".to_string());
        }
        return Ok(());
    }

    // ── Session Export ──────────────────────────────────────────────────────
    if input == "/export" || input.starts_with("/export ") {
        let path_override = input.strip_prefix("/export ").map(|s| s.trim());
        
        let export_messages: Vec<ExportMessage> = app.messages.iter().map(|m| ExportMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            images: m.images.clone(),
            reasoning: m.reasoning.clone(),
            timestamp: m.timestamp,
        }).collect();

        let export_branches: Vec<ExportBranch> = app.branches.iter().map(|b| ExportBranch {
            name: b.name.clone(),
            messages: b.messages.iter().map(|m| ExportMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                images: m.images.clone(),
                reasoning: m.reasoning.clone(),
                timestamp: m.timestamp,
            }).collect(),
            created_at: b.created_at,
        }).collect();

        let export = SessionExport::from_tui_state(
            app.session_id.clone(),
            app.model.clone(),
            export_messages,
            export_branches,
            app.tokens_used,
            app.tool_calls_count,
        );

        let result = if let Some(path) = path_override {
            export.save_to_file(path).map(|_| path.to_string())
        } else {
            export_to_default(&export).map(|p| p.to_string_lossy().to_string())
        };

        match result {
            Ok(path) => app.add_system_message(format!("💾 Session exported to: {}", path)),
            Err(e) => app.add_system_message(format!("❌ Export failed: {}", e)),
        }
        return Ok(());
    }

    // ── Session Import ──────────────────────────────────────────────────────
    if input.starts_with("/import ") {
        let path = input.strip_prefix("/import ").unwrap_or("").trim();
        match SessionExport::load_from_file(path) {
            Ok(export) => {
                // Convert export messages to ChatMessages
                let imported_messages: Vec<ChatMessage> = export.messages.iter().map(|m| ChatMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    images: m.images.clone(),
                    timestamp: m.timestamp,
                    multi_model_responses: Vec::new(),
                    reasoning: m.reasoning.clone(),
                }).collect();

                // Convert export branches
                let imported_branches: Vec<SessionBranch> = export.branches.iter().map(|b| SessionBranch {
                    name: b.name.clone(),
                    messages: b.messages.iter().map(|m| ChatMessage {
                        role: m.role.clone(),
                        content: m.content.clone(),
                        images: m.images.clone(),
                        timestamp: m.timestamp,
                        multi_model_responses: Vec::new(),
                        reasoning: m.reasoning.clone(),
                    }).collect(),
                    model_messages: Vec::new(),
                    created_at: b.created_at,
                }).collect();

                app.messages = imported_messages;
                app.branches = imported_branches;
                let imported_model = export.model.clone();
                app.model = imported_model;
                app.tokens_used = export.metadata.tokens_used;
                app.tool_calls_count = export.metadata.tool_calls_count;
                app.model_messages = export.to_model_messages();
                app.scroll = 0;

                app.add_system_message(format!(
                    "📂 Session imported from {} (v{}, exported {})",
                    path,
                    export.version,
                    export.exported_at.format("%Y-%m-%d %H:%M:%S")
                ));
                app.add_system_message(format!(
                    "   Messages: {} | Branches: {} | Model: {}",
                    export.messages.len(),
                    export.branches.len(),
                    export.model
                ));
            }
            Err(e) => app.add_system_message(format!("❌ Import failed: {}", e)),
        }
        return Ok(());
    }

    if input == "/imports" || input == "/list-exports" {
        match list_exports() {
            Ok(exports) if exports.is_empty() => {
                app.add_system_message("No exported sessions found.".to_string());
            }
            Ok(exports) => {
                app.add_system_message(format!("📂 Exported sessions ({} found):", exports.len()));
                for (i, (path, export)) in exports.iter().take(10).enumerate() {
                    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
                    app.add_system_message(format!(
                        "  {}. {} — {} messages, {} branches — {}",
                        i + 1,
                        filename,
                        export.messages.len(),
                        export.branches.len(),
                        export.exported_at.format("%Y-%m-%d %H:%M")
                    ));
                }
                app.add_system_message("Use /import <path> to load one.".to_string());
            }
            Err(e) => app.add_system_message(format!("❌ Failed to list exports: {}", e)),
        }
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
        // Track tokens for swarm commands too
        app.tokens_used += input.len() as u64 / 4;

        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts.get(1).copied().unwrap_or("status");
        let prompt = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();

        match cmd {
            "init" => {
                if prompt.is_empty() {
                    app.add_system_message("Usage: /swarm init <seed prompt>".to_string());
                    app.add_system_message("Example: /swarm init Build a REST API with auth".to_string());
                } else {
                    // Reload config from disk to pick up any edits
                    let fresh_config = crate::config::Config::load_or_default().unwrap_or_else(|_| app.config.clone());
                    // Update cached config
                    app.config = fresh_config.clone();
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
                        Ok(()) => {
                            app.swarm_running = true;
                            // Subscribe to swarm activity events
                            app.swarm_event_rx = Some(engine.subscribe());
                            // Initialize swarm_agents so we can detect state changes
                            app.swarm_agents = engine.agent_snapshot().await;
                            app.add_system_message("🐝 Swarm loop started. Agents are working...".to_string());
                        }
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
                    app.add_system_message(format!("Config: max_agents={}, roles={:?}",
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
        let path_str = input.strip_prefix("/image ").unwrap_or("").trim();
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

    if input == "/lint" {
        app.add_system_message("🔍 Running linter...".to_string());
        let path = app.config.filesystem.working_directory.clone()
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| ".".to_string());
        match crate::linting::detect_linter(&path) {
            Some(linter) => {
                app.add_system_message(format!("Detected linter: {}", linter));
                match crate::linting::run_linter(&path).await {
                    Ok(results) => {
                        if results.is_empty() {
                            app.add_system_message("✅ No issues found!".to_string());
                        } else {
                            let summary = results.iter()
                                .map(|r| format!("[{}] {}:{} — {}", r.severity, r.file, r.line, r.message))
                                .collect::<Vec<_>>()
                                .join("\n");
                            let errors = results.iter().filter(|r| r.severity == crate::linting::Severity::Error).count();
                            let warnings = results.iter().filter(|r| r.severity == crate::linting::Severity::Warning).count();
                            app.add_system_message(format!(
                                "🔍 Linter results ({} errors, {} warnings):\n```\n{}\n```",
                                errors, warnings, summary
                            ));
                        }
                    }
                    Err(e) => app.add_system_message(format!("❌ Linter failed: {}", e)),
                }
            }
            None => app.add_system_message("❌ No supported linter detected.".to_string()),
        }
        return Ok(());
    }

    if input == "/repo-map" || input == "/map" {
        let path = app.config.filesystem.working_directory.clone()
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| ".".to_string());
        app.add_system_message(format!("🗺️ Building repo map for {}...", path));
        match crate::repo_map::build_repo_map(&path) {
            Ok(map) => {
                let compact = crate::repo_map::format_repo_map_compact(&map);
                app.add_system_message(format!("🗺️ Repo Map:\n```\n{}\n```", compact));
            }
            Err(e) => app.add_system_message(format!("❌ Repo map failed: {}", e)),
        }
        return Ok(());
    }

    if input == "/yolo" {
        app.yolo_mode = !app.yolo_mode;
        let status = if app.yolo_mode { "ON ✅" } else { "OFF ❌" };
        app.add_system_message(format!("🤘 YOLO mode is {} — tool calls will {}be auto-approved.", status, if app.yolo_mode { "" } else { "NOT " }));
        return Ok(());
    }

    if input == "/checkpoint" || input.starts_with("/checkpoint ") {
        let name = input.strip_prefix("/checkpoint").unwrap_or("").trim();
        let name = if name.is_empty() {
            format!("checkpoint-{}", chrono::Local::now().format("%H%M%S"))
        } else {
            name.to_string()
        };
        match app.checkpoint_stack.save(&name) {
            Ok(_) => app.add_system_message(format!("💾 Checkpoint saved: {}", name)),
            Err(e) => app.add_system_message(format!("❌ Checkpoint failed: {}", e)),
        }
        return Ok(());
    }

    if input == "/undo" {
        match app.checkpoint_stack.undo() {
            Ok(name) => app.add_system_message(format!("↩️ Restored checkpoint: {}", name)),
            Err(e) => app.add_system_message(format!("❌ Undo failed: {}", e)),
        }
        return Ok(());
    }

    if input == "/redo" {
        match app.checkpoint_stack.redo() {
            Ok(name) => app.add_system_message(format!("↪️ Restored checkpoint: {}", name)),
            Err(e) => app.add_system_message(format!("❌ Redo failed: {}", e)),
        }
        return Ok(());
    }

    if input == "/diff" || input.starts_with("/diff ") {
        let path = input.strip_prefix("/diff").unwrap_or("").trim();
        let path = if path.is_empty() { "." } else { path };
        match std::process::Command::new("git")
            .args(["diff", "--stat", path])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim().is_empty() {
                    app.add_system_message("✅ No uncommitted changes.".to_string());
                } else {
                    // Show stat summary
                    app.add_system_message(format!("📊 Changes since last checkpoint:\n{}", stdout.trim()));
                    // Also show full diff if it's not too large
                    let full = std::process::Command::new("git")
                        .args(["diff", path])
                        .output();
                    if let Ok(full_out) = full {
                        let full_diff = String::from_utf8_lossy(&full_out.stdout);
                        if full_diff.len() > 8000 {
                            app.add_system_message(format!("📄 Full diff ({} bytes) — use `TOOL:terminal git diff` for complete output", full_diff.len()));
                        } else if !full_diff.trim().is_empty() {
                            app.add_system_message(format!("```\n{}\n```", full_diff.trim()));
                        }
                    }
                }
            }
            Err(e) => app.add_system_message(format!("❌ Failed to run git diff: {}", e)),
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

    // ── Direct tool routing for common commands ─────────────────────────────
    // If user types exactly "test", route directly to test tool
    if input.trim().eq_ignore_ascii_case("test") {
        let project_path = app
            .config
            .filesystem
            .working_directory
            .clone()
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| "/home/synth".to_string());
        app.add_user_message(input.clone());
        let test_tool = crate::tools::test_runner::TestTool;
        match crate::tools::Tool::execute(&test_tool, &format!("run {}", project_path)) {
            Ok(result) => {
                app.add_system_message(result.clone());
                app.model_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Test results: {}", result),
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                });
            }
            Err(e) => app.add_system_message(format!("Test error: {}", e)),
        }
        return Ok(());
    }

    app.add_user_message(input.clone());
    // Reset circuit breaker on fresh user input
    app.empty_response_count = 0;

    // ── Swarm Guard: Block regular chat while swarm is working ─────────────
    if app.swarm_running {
        app.add_system_message(
            "⏸ Swarm is active. Regular chat is paused while agents work.\n\
             Use `/swarm status` for progress or `/swarm stop` to halt agents."
                .to_string(),
        );
        return Ok(());
    }

    if input.starts_with("agent:") {
        let task = input.strip_prefix("agent:").unwrap_or("").trim();
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
                        app.add_assistant_message(response, None);
                    }
                    Err(e) => app.add_system_message(format!("Agent error: {}", e)),
                }
            }
            Err(e) => app.add_system_message(format!("Failed to initialize agent: {}", e)),
        }

        app.mode = AppMode::Normal;
        return Ok(());
    }

    if input.starts_with("TOOL:") || input.starts_with("TOOL.") {
        handle_user_tool_invocation(app, &input)?;
        return Ok(());
    }

    // Spawn model response in background so the user message appears immediately.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.stream_rx = Some(rx);

    // ── Phase 1: Enrich system prompt with memory + skills ────────────────
    let mut model_messages = app.model_messages.clone();

    // ── Phase 1b: Context compression if threshold exceeded ────────────────
    // HARD GUARDRAIL: If estimated tokens exceed 95% of context window,
    // force compression regardless of threshold. If compression fails,
    // truncate oldest messages to prevent API errors.
    let compression_notice = if let Some(ref mut compressor) = app.compressor {
        let estimated = crate::memory::compression::estimate_tokens(&model_messages);
        let threshold_trigger = compressor.should_compress(estimated, app.model_context_length);
        let hard_limit_trigger = estimated > (app.model_context_length * 95 / 100);

        if threshold_trigger || hard_limit_trigger {
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
                Ok(false) if hard_limit_trigger => {
                    // Compression didn't fire but we're at hard limit — emergency truncate
                    let preserve_count = (app.model_context_length * 90 / 100).max(1000);
                    emergency_truncate_messages(&mut model_messages, preserve_count);
                    Some(format!(
                        "⚠️ Context emergency-truncated: exceeded {} tokens ({}% of {} limit). Oldest messages removed.",
                        estimated,
                        estimated * 100 / app.model_context_length,
                        app.model_context_length
                    ))
                }
                Ok(false) => None,
                Err(e) => {
                    if hard_limit_trigger {
                        let preserve_count = (app.model_context_length * 90 / 100).max(1000);
                        emergency_truncate_messages(&mut model_messages, preserve_count);
                    }
                    Some(format!(
                        "⚠️ Context compression failed: {}",
                        e
                    ))
                }
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

/// Extract thinking content from a streaming chunk.
/// Returns (Option<reasoning_text>, remaining_content).
/// Handles partial <think> tags across chunk boundaries.
fn extract_thinking_from_chunk(chunk: &str) -> (Option<String>, String) {
    let mut reasoning = String::new();
    let mut content = String::new();
    let mut remaining = chunk;

    while !remaining.is_empty() {
        if let Some(start) = remaining.find("<think>") {
            // Add text before <think> to content
            content.push_str(&remaining[..start]);
            let after_start = &remaining[start + 7..];
            if let Some(end) = after_start.find("</think>") {
                // Complete think block
                reasoning.push_str(&after_start[..end]);
                remaining = &after_start[end + 8..];
            } else {
                // Incomplete think block — rest is reasoning
                reasoning.push_str(after_start);
                remaining = "";
            }
        } else {
            // No more think tags
            content.push_str(remaining);
            remaining = "";
        }
    }

    let reasoning_opt = if reasoning.is_empty() { None } else { Some(reasoning) };
    (reasoning_opt, content)
}

/// Parse all explicit TOOL: invocations from text, anywhere in the response.
/// Handles both `TOOL:tool_name args` and `TOOL: tool_name args` (with space after colon).
fn parse_embedded_tools(text: &str) -> Vec<(String, String)> {
    let mut tools = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        // Handle both "TOOL:tool_name args" and "TOOL.tool_name args"
        let prefix = if trimmed.starts_with("TOOL:") {
            Some("TOOL:")
        } else if trimmed.starts_with("TOOL.") {
            Some("TOOL.")
        } else {
            None
        };
        if let Some(p) = prefix {
            let rest = &trimmed[p.len()..]; // after "TOOL:" or "TOOL."
            let rest = rest.trim_start(); // handle "TOOL: fs cat" → "fs cat"
            // Try JSON format first: tool_name:0>{"args":"query"}
            if let Some((name, args)) = parse_json_tool_format(rest) {
                tools.push((name, args));
            } else {
                // Original space-split format: tool_name args
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if !parts.is_empty() && !parts[0].is_empty() {
                    let tool_name = parts[0].trim().to_string();
                    let args = parts.get(1).unwrap_or(&"").trim().to_string();
                    tools.push((tool_name, args));
                }
            }
        }
    }
    tools
}

/// Parse JSON tool format: supports two forms:
/// 1. tool_name {"key": "value", ...}    (bare JSON)
/// 2. tool_name:0>{"key": "value", ...}  (numeric-indexed)
fn parse_json_tool_format(rest: &str) -> Option<(String, String)> {
    // Try bare JSON format first: tool_name {"key": "value"}
    if let Some(space_pos) = rest.find(['{', ':'])
        && rest.as_bytes().get(space_pos) == Some(&b'{') {
            let tool_name = rest[..space_pos].trim().to_string();
            if tool_name.is_empty() {
                return None;
            }
            let json_str = find_balanced_json(&rest[space_pos..])?;
            return extract_args_from_json(json_str, &tool_name);
        }

    // Try numeric-indexed format: tool_name:0>{"key": "value"}
    let colon_pos = rest.find(':')?;
    let after_colon = &rest[colon_pos + 1..];
    if after_colon.is_empty() || !after_colon.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let gt_pos = after_colon.find('>')?;
    if !after_colon[gt_pos..].starts_with(">{") {
        return None;
    }

    let tool_name = rest[..colon_pos].trim().to_string();
    let json_str = find_balanced_json(&after_colon[gt_pos + 1..])?;
    extract_args_from_json(json_str, &tool_name)
}

/// Find balanced JSON string starting with '{' or '['.
fn find_balanced_json(s: &str) -> Option<&str> {
    if s.is_empty() {
        return None;
    }
    let first_char = s.chars().next()?;
    let (open, close) = match first_char {
        '{' => ('{', '}'),
        '[' => ('[', ']'),
        _ => return None,
    };
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if !in_string {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
        }
    }
    None
}

/// Extract args from a parsed JSON object string.
/// Maps JSON fields to space-separated args that each tool's execute method expects.
fn extract_args_from_json(json_str: &str, tool_name: &str) -> Option<(String, String)> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(args) = v.get("args").and_then(|a| a.as_str()) {
            return Some((tool_name.to_string(), args.to_string()));
        }
        if let Some(args) = v.get("query").and_then(|a| a.as_str()) {
            return Some((tool_name.to_string(), args.to_string()));
        }

        let fields: &[&str] = match tool_name {
            "fs" => &["operation", "path", "content"],
            "git" => &["command", "args"],
            "search" => &["query", "path"],
            "grep" => &["pattern", "path"],
            "terminal" => &["command"],
            "edit" => &["file", "old_string", "new_string"],
            "test" => &["path", "framework"],
            "refactor" => &["operation", "file", "line", "column", "new_name"],
            "lsp" => &["command", "file", "line", "column"],
            _ => return extract_generic_args(v, tool_name),
        };

        let mut parts: Vec<String> = Vec::with_capacity(fields.len());
        for field in fields {
            if let Some(val) = v.get(*field) {
                match val {
                    serde_json::Value::String(s) => parts.push(s.clone()),
                    serde_json::Value::Number(n) => parts.push(n.to_string()),
                    _ => {}
                }
            }
        }

        if parts.is_empty() {
            return None;
        }

        if tool_name == "edit" && !parts.is_empty() {
            let file = parts.first().cloned().unwrap_or_default();
            if file.is_empty() {
                return None;
            }
            let old_str = parts.get(1).cloned().unwrap_or_default();
            let new_str = parts.get(2).cloned().unwrap_or_default();
            if !old_str.is_empty() {
                return Some((tool_name.to_string(), format!("replace {}\n{}\n---\n{}", file, old_str, new_str)));
            }
            return Some((tool_name.to_string(), format!("read {}", file)));
        }
        if tool_name == "test" && !parts.is_empty() {
            let mut reordered = vec!["run".to_string()];
            reordered.extend(parts);
            return Some((tool_name.to_string(), reordered.join(" ")));
        }

        Some((tool_name.to_string(), parts.join(" ")))
    } else {
        None
    }
}

/// Generic fallback for unknown/MCP tools: collect all string/number values.
fn extract_generic_args(v: serde_json::Value, tool_name: &str) -> Option<(String, String)> {
    if let Some(obj) = v.as_object() {
        let parts: Vec<String> = obj
            .values()
            .filter_map(|val| match val {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            return Some((tool_name.to_string(), parts.join(" ")));
        }
    }
    None
}

/// Strip TOOL: / TOOL. lines from assistant content for display.
fn strip_tool_lines(text: &str) -> String {
    let mut result = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("TOOL:") && !trimmed.starts_with("TOOL.") {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
        }
    }
    result
}

/// Extract thinking/reasoning content from <think>...</think> blocks.
fn extract_thinking(text: &str) -> String {
    let start = text.find("<think>");
    let end = text.find("</think>");
    match (start, end) {
        (Some(s), Some(e)) if e > s => {
            text[s + 7..e].trim().to_string()
        }
        _ => String::new(),
    }
}

/// Strip all <think>...</think> blocks from text.
fn strip_think_tags(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result.find("</think>") {
            result.replace_range(start..end + 8, "");
        } else {
            break;
        }
    }
    result.trim().to_string()
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
    tool_call_id: None,
    tool_calls: None,
    reasoning_content: None,
    });

    let executor = AsyncToolExecutor::new();

        let total = tools.len();
    let mut batch_results: Vec<ToolResultEntry> = Vec::with_capacity(total);

for (idx, (tool_name, args)) in tools.iter().enumerate() {
        // Show progress indicator (throttled — every 5th tool + first + last)
        if idx == 0 || idx % 5 == 0 || idx == total - 1 {
            let _ = tx.send(StreamEvent::SystemMessage(format!(
                "🔧 Tool {}/{}: {} …",
                idx + 1, total, tool_name
            )));
        }

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
                // Collect for batched display
                batch_results.push(ToolResultEntry {
                    name: tool_name.clone(),
                    args: args.clone(),
                    result: sanitized.clone(),
                    success: true,
                });
                follow_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool result ({} {}): {}", tool_name, args, sanitized),
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                });
            }
            Err(e) => {
                batch_results.push(ToolResultEntry {
                    name: tool_name.clone(),
                    args: args.clone(),
                    result: e.to_string(),
                    success: false,
                });
                follow_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool error ({} {}): {}", tool_name, args, e),
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                });
            }
        }
    }
    // Send batched results for collapsed display
    let _ = tx.send(StreamEvent::ToolResultsBatch {
        results: batch_results,
    });
    // Follow-up request with all tool results — inject completion mandate
    // Build a summary of all tool results to ensure the model sees them clearly
    let mut tool_summary = String::from("TOOL EXECUTION COMPLETE. Here are the results:\n\n");
    for (idx, (tool_name, args)) in tools.iter().enumerate() {
        // Find the result for this tool from follow_messages
        let result_prefix = format!("Tool result ({} {}):", tool_name, args);
        if let Some(msg) = follow_messages.iter().find(|m| m.role == "user" && m.content.starts_with(&result_prefix)) {
            tool_summary.push_str(&format!("{}. {}\n{}", idx + 1, tool_name, msg.content));
            tool_summary.push_str("\n\n");
        } else if let Some(msg) = follow_messages.iter().find(|m| m.role == "user" && m.content.starts_with(&format!("Tool error ({} {}):", tool_name, args))) {
            tool_summary.push_str(&format!("{}. {}\n{}", idx + 1, tool_name, msg.content));
            tool_summary.push_str("\n\n");
        }
    }
    tool_summary.push_str("Based on these tool results, provide a complete response. Explain what was found, what it means, and what to do next. If a tool result is empty or missing, state that explicitly — do not hallucinate.");

    follow_messages.push(Message {
        role: "user".to_string(),
        content: tool_summary,
        images: None,
    tool_call_id: None,
    tool_calls: None,
    reasoning_content: None,
    });

    let follow_up = ChatRequest::new(model.to_string(), follow_messages.clone(), true);
    match tokio::time::timeout(Duration::from_secs(120), provider.chat_stream(follow_up)).await {
        Ok(Ok((follow_chunks, _metrics))) => {
            let follow_content: String = follow_chunks.join("");
            let trimmed = follow_content.trim();
            if trimmed.is_empty() {
                let _ = tx.send(StreamEvent::Error(
                    "Model returned empty follow-up after tool execution. Re-prompting...".to_string()
                ));
                // Re-prompt with stronger mandate — include tool results so model sees them
                let mut retry_messages = follow_messages.clone();
                retry_messages.push(Message {
                    role: "user".to_string(),
                    content: "Your previous response was empty. Using the tool results already provided above, write a complete response explaining what was found, what it means, and the next step. Do not skip this.".to_string(),
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                });
                let retry_req = ChatRequest::new(model.to_string(), retry_messages, true);
                match tokio::time::timeout(Duration::from_secs(120), provider.chat_stream(retry_req)).await {
                    Ok(Ok((retry_chunks, _retry_metrics))) => {
                        let retry_content: String = retry_chunks.join("");
                        let _ = tx.send(StreamEvent::FollowUp(retry_content));
                        let _ = tx.send(StreamEvent::Done);
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(StreamEvent::Error(format!("Retry follow-up failed: {}", e)));
                        let _ = tx.send(StreamEvent::Done);
                    }
                    Err(_) => {
                        let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));
                        let _ = tx.send(StreamEvent::Done);
                    }
                }
            } else if trimmed.split_whitespace().count() < 15 {
                // Too short — treat as incomplete and re-prompt
                let _ = tx.send(StreamEvent::SystemMessage(
                    "⚠️ Follow-up too brief — requesting complete response...".to_string()
                ));
                let mut retry_messages = follow_messages.clone();
                retry_messages.push(Message {
                    role: "assistant".to_string(),
                    content: follow_content.clone(),
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                });
                retry_messages.push(Message {
                    role: "user".to_string(),
                    content: "That was too brief. Using the tool results already provided above, write a thorough response: what did the tools reveal, what does it mean, and what's next?".to_string(),
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                });
                let retry_req = ChatRequest::new(model.to_string(), retry_messages, true);
                match tokio::time::timeout(Duration::from_secs(120), provider.chat_stream(retry_req)).await {
                    Ok(Ok((retry_chunks, _retry_metrics))) => {
                        let retry_content: String = retry_chunks.join("");
                        let _ = tx.send(StreamEvent::FollowUp(retry_content));
                        let _ = tx.send(StreamEvent::Done);
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(StreamEvent::Error(format!("Retry follow-up failed: {}", e)));
                        let _ = tx.send(StreamEvent::Done);
                    }
                    Err(_) => {
                        let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));
                        let _ = tx.send(StreamEvent::Done);
                    }
                }
            } else {
                let _ = tx.send(StreamEvent::FollowUp(follow_content));
            }
            let _ = tx.send(StreamEvent::Done);
        }
        Ok(Err(e)) => {
            let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
            let _ = tx.send(StreamEvent::Done);
        }
        Err(_) => {
            let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));
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
    // Attach OpenAI-compatible tool definitions so the model knows it can call tools
    request.tools = Some(crate::tools::get_openai_tool_definitions());

    let _secondary_providers: Vec<(String, Provider)> = if is_multi_model {
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

    match provider.chat_stream_realtime(request).await {
        Ok((mut chunk_rx, mut metrics)) => {
            let mut full_content = String::new();
            let mut accumulated_reasoning = String::new();
            let stream_start = std::time::Instant::now();
            let mut first_token_time: Option<std::time::Instant> = None;
            let mut token_count: u32 = 0;

            // Read chunks from the provider in real-time and forward to TUI
            while let Some(chunk) = chunk_rx.recv().await {
                match chunk {
                    StreamChunk::Reasoning(r) => {
                        accumulated_reasoning.push_str(&r);
                        let _ = tx.send(StreamEvent::ReasoningChunk(r));
                    }
                    StreamChunk::Content(c) => {
                        if first_token_time.is_none() {
                            first_token_time = Some(std::time::Instant::now());
                        }
                        full_content.push_str(&c);
                        token_count += 1;
                        let _ = tx.send(StreamEvent::Chunk(c));
                    }
                    StreamChunk::ToolCall { id, name, arguments: args } => {
                        // Native tool call from the model — execute it directly
                        let _ = tx.send(StreamEvent::SystemMessage(format!(
                            "🔧 Tool call: {} {}",
                            name, args
                        )));

                        let tool_args = extract_args_from_json(&args, &name)
                            .map(|(_, extracted)| extracted)
                            .unwrap_or_else(|| args.clone());

                        match security_engine.check_tool_call(&name, &tool_args) {
                            crate::security::SecurityDecision::Allow => {
                                let executor = AsyncToolExecutor::new();
                                match executor
                                    .execute_with_timeout_simple(
                                        name.clone(),
                                        tool_args.clone(),
                                        30000,
                                    )
                                    .await
                                {
                                    Ok(result) => {
                                        let sanitized = security_engine.sanitize_output(&name, &result);
                                        let _ = tx.send(StreamEvent::ToolResult {
                                            name: name.clone(),
                                            args: tool_args.clone(),
                                            result: sanitized.clone(),
                                            success: true,
                                        });

                                        // Build follow-up messages with tool result
                                        let mut follow_messages = model_messages.clone();
                                        // Ensure we have a valid tool_call_id — Kimi streams may omit it
                                        let call_id = if id.is_empty() {
                                            Uuid::new_v4().to_string()
                                        } else {
                                            id.clone()
                                        };
                                        // Assistant message MUST include tool_calls array with the id
                                        // Reasoning is kept ephemeral — not stored in persistent history
                                        follow_messages.push(Message {
                                            role: "assistant".to_string(),
                                            content: full_content.clone(),
                                            images: None,
                                            tool_call_id: None,
                                            tool_calls: Some(vec![
                                                crate::providers::ToolCallRequest {
                                                    id: call_id.clone(),
                                                    r#type: "function".to_string(),
                                                    function: crate::providers::ToolCallFunction {
                                                        name: name.clone(),
                                                        arguments: args.clone(),
                                                    },
                                                },
                                            ]),
                                            reasoning_content: None,
                                        });
                                        // Tool result message MUST include tool_call_id matching the assistant's tool_calls
                                        follow_messages.push(Message {
                                            role: "tool".to_string(),
                                            content: sanitized,
                                            images: None,
                                            tool_call_id: Some(call_id.clone()),
                                            tool_calls: None,
                                            reasoning_content: None,
                                        });

                                        let mut follow_up = ChatRequest::new(
                                            model.clone(),
                                            follow_messages.clone(),
                                            true,
                                        );
                                        follow_up.tools = Some(crate::tools::get_openai_tool_definitions());

                                        match provider.chat_stream_realtime(follow_up).await {
                                            Ok((mut follow_rx, _)) => {
                                                let mut follow_content = String::new();
                                                while let Some(fchunk) = follow_rx.recv().await {
                                                    match fchunk {
                                                        StreamChunk::Content(c) => {
                                                            follow_content.push_str(&c);
                                                            let _ = tx.send(StreamEvent::Chunk(c));
                                                        }
                                                        StreamChunk::Reasoning(r) => {
                                                            let _ = tx.send(StreamEvent::ReasoningChunk(r));
                                                        }
                                                        _ => {}
                                                    }
                                                }
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
                                            name,
                                            args: tool_args,
                                            result: e.to_string(),
                                            success: false,
                                        });
                                        let _ = tx.send(StreamEvent::Done);
                                    }
                                }
                            }
                            crate::security::SecurityDecision::Deny { reason } => {
                                let _ = tx.send(StreamEvent::SystemMessage(format!(
                                    "🚫 Tool '{}' blocked: {}",
                                    name, reason
                                )));
                                let _ = tx.send(StreamEvent::Done);
                            }
                            crate::security::SecurityDecision::RequireApproval { reason: _, risk_level: _ } => {
                                let _ = tx.send(StreamEvent::SystemMessage(format!(
                                    "⏸️ Tool '{}' requires approval (not yet implemented for native tool calls)",
                                    name
                                )));
                                let _ = tx.send(StreamEvent::Done);
                            }
                        }
                    }
                    StreamChunk::Finish(fr) => {
                        if fr == "stop" {
                            // Normal completion — fall through to existing tool detection
                        }
                    }
                }
            }

            // Compute actual metrics now that streaming is done
            let total_latency = stream_start.elapsed();
            let first_token_latency = first_token_time
                .map(|t| t.duration_since(stream_start))
                .unwrap_or_default();
            metrics.first_token_latency_ms = first_token_latency.as_millis() as u64;
            metrics.total_latency_ms = total_latency.as_millis() as u64;
            metrics.tokens_generated = token_count;

            let _ = tx.send(StreamEvent::ResponseComplete {
                content: full_content.clone(),
                metrics,
            });

            // Handle tool invocation + follow-up using the accumulated full_content
            let embedded_tools = parse_embedded_tools(&full_content);
            if !embedded_tools.is_empty() {
                let _ = execute_tool_chain(
                    &tx, &provider, &model, &model_messages, &security_engine, &embedded_tools, &full_content
                ).await;
            } else {
                let suggestions = crate::tools::detect_tool_suggestions(&full_content);
                let high_conf: Vec<_> = suggestions.into_iter().filter(|s| s.confidence >= 0.6).collect();
                if high_conf.len() > 1 {
                    // Multi-file edit batch — send to UI for approval
                    let _ = tx.send(StreamEvent::SystemMessage(format!(
                        "🔧 Batch of {} tool suggestions detected. Review and approve.",
                        high_conf.len()
                    )));
                    let _ = tx.send(StreamEvent::SetPendingBatch(crate::tools::ToolBatch::new(high_conf)));
                } else if let Some(suggestion) = high_conf.into_iter().next() {
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

                                    let mut follow_messages = model_messages.clone();
                                    follow_messages.push(Message {
                                        role: "assistant".to_string(),
                                        content: full_content.clone(),
                                        images: None,
                                    tool_call_id: None,
                                    tool_calls: None,
                                    reasoning_content: None,
                                    });
                                    follow_messages.push(Message {
                                        role: "user".to_string(),
                                        content: format!("Tool result: {}", sanitized),
                                        images: None,
                                    tool_call_id: None,
                                    tool_calls: None,
                                    reasoning_content: None,
                                    });
                                    follow_messages.push(Message {
                                        role: "user".to_string(),
                                        content: "TOOL EXECUTION COMPLETE. Based on the tool result provided above, write a complete response. Explain what was found, what it means, and what to do next.".to_string(),
                                        images: None,
                                    tool_call_id: None,
                                    tool_calls: None,
                                    reasoning_content: None,
                                    });

                                    let follow_up = ChatRequest::new(
                                        model.clone(),
                                        follow_messages.clone(),
                                        true,
                                    );

                                    match provider.chat_stream(follow_up).await {
                                        Ok((follow_chunks, _metrics)) => {
                                            let follow_content: String = follow_chunks.join("");
                                            let trimmed = follow_content.trim();
                                            if trimmed.is_empty() {
                                                let _ = tx.send(StreamEvent::Error(
                                                    "Empty follow-up after tool execution. Re-prompting...".to_string()
                                                ));
                                                let mut retry = follow_messages.clone();
                                                retry.push(Message {
                                                    role: "user".to_string(),
                                                    content: "Your previous response was empty. Using the tool result already provided above, write a complete response explaining what was found, what it means, and the next step.".to_string(),
                                                    images: None,
                                                tool_call_id: None,
                                                tool_calls: None,
                                                reasoning_content: None,
                                                });
                                                let retry_req = ChatRequest::new(model.clone(), retry, true);
                                                match provider.chat_stream(retry_req).await {
                                                    Ok((rc, _)) => {
                                                        let _ = tx.send(StreamEvent::FollowUp(rc.join("")));
                                                        let _ = tx.send(StreamEvent::Done);
                                                    }
                                                    Err(e) => {
                                                        let _ = tx.send(StreamEvent::Error(format!("Retry failed: {}", e)));
                                                        let _ = tx.send(StreamEvent::Done);
                                                    }
                                                }
                                            } else {
                                                let _ = tx.send(StreamEvent::FollowUp(follow_content));
                                            }
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
                        crate::security::SecurityDecision::Deny { reason } => {
                            let _ = tx.send(StreamEvent::SystemMessage(format!(
                                "🚫 Tool '{}' blocked: {}",
                                suggestion.tool_name, reason
                            )));
                            let _ = tx.send(StreamEvent::Done);
                        }
                        crate::security::SecurityDecision::RequireApproval { reason: _, risk_level: _ } => {
                            let _ = tx.send(StreamEvent::SetPendingSuggestion(suggestion));
                            let _ = tx.send(StreamEvent::Done);
                        }
                    }
                } else {
                    let _ = tx.send(StreamEvent::Done);
                }
            }
        }
        Err(e) => {
            let _ = tx.send(StreamEvent::Error(format!("Stream error: {}", e)));
            let _ = tx.send(StreamEvent::Done);
            return Ok(());
        }
    }

    Ok(())
}

#[allow(dead_code)]
async fn stream_model_response_task_legacy(
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

                // Check if this chunk is reasoning content (wrapped in think tags)
                if chunk.starts_with("<think>") && chunk.ends_with("</think>") {
                    // Extract the inner reasoning text and send as ReasoningChunk
                    let inner = &chunk[7..chunk.len()-8]; // strip <think> and </think>
                    let _ = tx.send(StreamEvent::ReasoningChunk(inner.to_string()));
                } else {
                    let _ = tx.send(StreamEvent::Chunk(chunk.clone()));
                }
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
                                    tool_call_id: None,
                                    tool_calls: None,
                                    reasoning_content: None,
                                    });
                                    follow_messages.push(Message {
                                        role: "user".to_string(),
                                        content: format!("Tool result: {}", sanitized),
                                        images: None,
                                    tool_call_id: None,
                                    tool_calls: None,
                                    reasoning_content: None,
                                    });
                                    follow_messages.push(Message {
                                        role: "user".to_string(),
                                        content: "TOOL EXECUTION COMPLETE. Based on the tool result provided above, write a complete response. Explain what was found, what it means, and what to do next.".to_string(),
                                        images: None,
                                    tool_call_id: None,
                                    tool_calls: None,
                                    reasoning_content: None,
                                    });

                                    let follow_up = ChatRequest::new(
                                        model.clone(),
                                        follow_messages.clone(),
                                        true,
                                    );

                                    match provider.chat_stream(follow_up).await {
                                        Ok((follow_chunks, _metrics)) => {
                                            let follow_content: String = follow_chunks.join("");
                                            let trimmed = follow_content.trim();
                                            if trimmed.is_empty() {
                                                let _ = tx.send(StreamEvent::Error(
                                                    "Empty follow-up after tool execution. Re-prompting...".to_string()
                                                ));
                                                let mut retry = follow_messages.clone();
                                                retry.push(Message {
                                                    role: "user".to_string(),
                                                    content: "Your previous response was empty. Using the tool result already provided above, write a complete response explaining what was found, what it means, and the next step.".to_string(),
                                                    images: None,
                                                tool_call_id: None,
                                                tool_calls: None,
                                                reasoning_content: None,
                                                });
                                                let retry_req = ChatRequest::new(model.clone(), retry, true);
                                                match provider.chat_stream(retry_req).await {
                                                    Ok((rc, _)) => {
                                                        let _ = tx.send(StreamEvent::FollowUp(rc.join("")));
                                                        let _ = tx.send(StreamEvent::Done);
                                                    }
                                                    Err(e) => {
                                                        let _ = tx.send(StreamEvent::Error(format!("Retry failed: {}", e)));
                                                        let _ = tx.send(StreamEvent::Done);
                                                    }
                                                }
                                            } else {
                                                let _ = tx.send(StreamEvent::FollowUp(follow_content));
                                            }
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
            let _ = tx.send(StreamEvent::Done);
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

/// Handle a slash command result from the registry.
/// Returns Ok(true) if fully handled, Ok(false) to fall through to legacy handlers.
async fn handle_slash_result(
    app: &mut App,
    result: crate::slash_commands::SlashResult,
    input: &str,
) -> Result<bool> {
    use crate::slash_commands::SlashResult;

    match result {
        SlashResult::Tool { name, args } => {
            app.add_system_message(format!("🔧 /{} {}", name, args));
            // Route to appropriate tool
            match name.as_str() {
                "git" => {
                    let git_tool = crate::tools::GitTool;
                    match git_tool.execute(&args) {
                        Ok(output) => app.add_system_message(format!(
                            "📦 git {}:\n```\n{}\n```",
                            args, output.trim()
                        )),
                        Err(e) => app.add_system_message(format!("❌ git {} failed: {}", args, e)),
                    }
                }
                "test" => {
                    let test_tool = crate::tools::test_runner::TestTool;
                    match crate::tools::Tool::execute(&test_tool, &args) {
                        Ok(result) => app.add_system_message(result),
                        Err(e) => app.add_system_message(format!("❌ Test failed: {}", e)),
                    }
                }
                "checkpoint" => {
                    let parts: Vec<&str> = args.splitn(2, ' ').collect();
                    let subcmd = parts.first().copied().unwrap_or("");
                    let rest = parts.get(1).copied().unwrap_or("");
                    match subcmd {
                        "save" => {
                            match crate::tools::save_checkpoint(rest) {
                                Ok(cp) => {
                                    app.checkpoint_stack.push(cp.clone());
                                    app.add_system_message(format!(
                                        "💾 Checkpoint saved: {} ({})",
                                        cp.name, cp.git_ref
                                    ));
                                }
                                Err(e) => app.add_system_message(format!("❌ Checkpoint failed: {}", e)),
                            }
                        }
                        "restore" => {
                            // Find checkpoint by name in undo stack
                            if let Some(idx) = app.checkpoint_stack.undo_stack.iter().rposition(|cp| cp.name == rest) {
                                let cp = app.checkpoint_stack.undo_stack.remove(idx);
                                match crate::tools::restore_checkpoint(&cp) {
                                    Ok(msg) => {
                                        app.checkpoint_stack.push_redo(cp);
                                        app.add_system_message(format!("⏪ {}", msg));
                                    }
                                    Err(e) => {
                                        app.checkpoint_stack.push(cp);
                                        app.add_system_message(format!("❌ Restore failed: {}", e));
                                    }
                                }
                            } else {
                                app.add_system_message(format!("❌ Checkpoint '{}' not found", rest));
                            }
                        }
                        _ => {
                            app.add_system_message(format!("💾 Checkpoint: {} (use 'save <name>' or 'restore <name>')", args));
                        }
                    }
                }
                "lint" => {
                    let path = if args.is_empty() { ".".to_string() } else { args.clone() };
                    app.add_system_message(format!("🔍 Running linter on {}...", path));
                    match crate::linting::run_linter(&path).await {
                        Ok(results) => {
                            if results.is_empty() {
                                app.add_system_message("✅ No linting issues found.".to_string());
                            } else {
                                let errors = results.iter().filter(|r| matches!(r.severity, crate::linting::Severity::Error)).count();
                                let warnings = results.iter().filter(|r| matches!(r.severity, crate::linting::Severity::Warning)).count();
                                app.add_system_message(format!(
                                    "📊 Lint results: {} errors, {} warnings ({} total)",
                                    errors, warnings, results.len()
                                ));
                                for r in results.iter().take(10) {
                                    app.add_system_message(format!(
                                        "{} {}:{} — {}: {}",
                                        r.severity, r.file, r.line, r.code.as_deref().unwrap_or(""), r.message
                                    ));
                                }
                                if results.len() > 10 {
                                    app.add_system_message(format!("... and {} more issues", results.len() - 10));
                                }
                            }
                        }
                        Err(e) => {
                            app.add_system_message(format!("❌ Linter failed: {}", e));
                        }
                    }
                }
                "repo_map" => {
                    match crate::repo_map::build_repo_map(&args) {
                        Ok(map) => {
                            let formatted = crate::repo_map::format_repo_map_compact(&map);
                            app.add_system_message(format!("🗺️ Repo Map:\n{}", formatted));
                        }
                        Err(e) => {
                            app.add_system_message(format!("❌ Repo map failed: {}", e));
                        }
                    }
                }
                "guardian" => {
                    let target = if args.is_empty() { "recent".to_string() } else { args.to_string() };
                    app.add_system_message(format!("🛡️  Guardian reviewing: {}...", target));
                    let provider = app.provider.clone();
                    let model = app.model.clone();
                    let project_path = app.project_path.clone();

                    match crate::guardian::review(&target, &project_path, provider, model).await {
                        Ok(report) => {
                            app.add_system_message(report.format());
                        }
                        Err(e) => {
                            app.add_system_message(format!("❌ Guardian review failed: {}", e));
                        }
                    }
                }
                "headless" => {
                    let provider = app.provider.clone();
                    let model = app.model.clone();
                    let task = args.clone();
                    let project_path = app.project_path.clone();

                    tokio::spawn(async move {
                        // Create a git worktree for isolated background execution
                        let worktree_path = match create_worktree(&project_path, &task).await {
                            Ok(path) => path,
                            Err(e) => {
                                tracing::error!("[headless] Worktree creation failed: {}", e);
                                // Fall back to running in-place
                                project_path.clone()
                            }
                        };

                        let config = crate::headless::HeadlessConfig {
                            task: format!("{}", task),
                            yolo: true,
                            json: false,
                            timeout_secs: 300,
                            max_turns: 50,
                            model: None,
                            output_file: None,
                        };
                        match crate::headless::run_headless(config, provider, model, None).await {
                            Ok(summary) => {
                                tracing::info!("[headless] Complete: {}", summary);
                                // Clean up worktree after completion
                                let _ = remove_worktree(&project_path, &worktree_path).await;
                            }
                            Err(e) => {
                                tracing::error!("[headless] Failed: {}", e);
                                let _ = remove_worktree(&project_path, &worktree_path).await;
                            }
                        }
                    });

                    app.add_system_message("🤖 Headless task spawned in background (worktree isolated).".to_string());
                }
                _ => {
                    app.add_system_message(format!("⚠️ Tool '/{}' not yet wired in registry", name));
                }
            }
            Ok(true)
        }
        SlashResult::Prompt(prompt) => {
            // Inject as a user message that triggers the model
            app.add_user_message(prompt.clone());
            // Reset circuit breaker
            app.empty_response_count = 0;
            // Let the normal flow handle it — but we already added the user message
            // so we need to trigger the model response here
            // For now, just show what would be sent
            app.add_system_message(format!("🤖 Prompt: {}", prompt));
            Ok(true)
        }
        SlashResult::Toggle { setting, value } => {
            match setting.as_str() {
                "plan_mode" => {
                    app.plan_mode = value;
                    app.rebuild_system_prompt();
                    app.add_system_message(format!(
                        "📋 Plan mode: {}",
                        if value { "ON — analyze only, no edits" } else { "OFF — full execution" }
                    ));
                }
                "compact_context" => {
                    app.compact_context();
                    app.add_system_message("🗜️ Context compacted — conversation summarized.".to_string());
                }
                "vim_mode" => {
                    app.vim_mode = !app.vim_mode;
                    app.add_system_message(format!(
                        "⌨️ Vim mode: {}",
                        if app.vim_mode { "ON" } else { "OFF" }
                    ));
                }
                "mouse" => {
                    app.mouse_enabled = !app.mouse_enabled;
                    if app.mouse_enabled {
                        app.mouse_state.enable();
                    } else {
                        app.mouse_state.disable();
                    }
                    app.add_system_message(format!(
                        "🖱️ Mouse support: {}",
                        if app.mouse_enabled { "ON" } else { "OFF" }
                    ));
                }
                "architect_mode" => {
                    let model = app.config.architect_model.clone().unwrap_or_else(|| app.config.default_model.clone());
                    app.model = model.clone();
                    app.add_system_message(format!(
                        "🏗️ Architect mode: using model {}",
                        model
                    ));
                }
                "editor_mode" => {
                    let model = app.config.editor_model.clone().unwrap_or_else(|| app.config.default_model.clone());
                    app.model = model.clone();
                    app.add_system_message(format!(
                        "📝 Editor mode: using model {}",
                        model
                    ));
                }
                mode_str if mode_str.starts_with("effort:") => {
                    let level = mode_str.strip_prefix("effort:").unwrap_or("medium");
                    app.config.effort_level = level.to_string();
                    app.rebuild_system_prompt();
                    app.add_system_message(format!(
                        "⚡ Effort level set to: {}. System prompt updated.",
                        level.to_uppercase()
                    ));
                }
                "autonomous" => {
                    app.autonomous_mode = value;
                    app.add_system_message(format!(
                        "🤖 Autonomous mode: {}",
                        if value { "ON" } else { "OFF" }
                    ));
                }
                "auto_context" => {
                    if let Some(ref mut engine) = app.context_mode_engine {
                        engine.config.enabled = value;
                        app.rebuild_system_prompt();
                        app.add_system_message(format!(
                            "📁 Auto-context mode: {} — {}",
                            if value { "ON" } else { "OFF" },
                            if value { "relevant files will be auto-identified for each query" } else { "relevant files will not be auto-identified" }
                        ));
                    } else {
                        app.add_system_message("❌ Auto-context mode not available — no project path set.".to_string());
                    }
                }
                "ctx_clear" => {
                    match app.smart_context.clear() {
                        Ok(msg) => {
                            app.rebuild_system_prompt();
                            app.add_system_message(msg);
                        }
                        Err(e) => app.add_system_message(format!("❌ Failed to clear pinned context: {}", e)),
                    }
                }
                "yolo" => {
                    app.yolo_mode = value;
                    app.add_system_message(format!(
                        "⚡ YOLO mode: {}",
                        if value { "ON" } else { "OFF" }
                    ));
                }
                "batch_approve_all" => {
                    if let Some(ref mut batch) = app.pending_batch {
                        let count = batch.len();
                        batch.approve_all();
                        app.add_system_message(format!(
                            "✅ Approved all {} suggestions in batch",
                            count
                        ));
                    } else {
                        app.add_system_message("📭 No pending batch to approve.".to_string());
                    }
                }
                "batch_reject_all" => {
                    if let Some(ref mut batch) = app.pending_batch {
                        let count = batch.len();
                        batch.reject_all();
                        app.add_system_message(format!(
                            "❌ Rejected all {} suggestions in batch",
                            count
                        ));
                        app.pending_batch = None;
                    } else {
                        app.add_system_message("📭 No pending batch to reject.".to_string());
                    }
                }
                _ if setting.starts_with("batch_approve:") => {
                    let idx_str = &setting["batch_approve:".len()..];
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if let Some(ref mut batch) = app.pending_batch {
                            if idx < batch.approved.len() {
                                let name = batch.suggestions[idx].tool_name.clone();
                                let args = batch.suggestions[idx].args.clone();
                                batch.approved[idx] = true;
                                app.add_system_message(format!(
                                    "✅ Approved suggestion {}: {} {}",
                                    idx, name, args
                                ));
                            }
                        }
                    }
                }
                _ if setting.starts_with("batch_reject:") => {
                    let idx_str = &setting["batch_reject:".len()..];
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if let Some(ref mut batch) = app.pending_batch {
                            if idx < batch.approved.len() {
                                batch.approved[idx] = false;
                                app.add_system_message(format!(
                                    "❌ Rejected suggestion {}", idx
                                ));
                            }
                        }
                    }
                }
                _ => {
                    app.add_system_message(format!(
                        "🔧 Toggled {} = {}",
                        setting, value
                    ));
                }
            }
            Ok(true)
        }
        SlashResult::SwitchMode(mode_str) => {
            if let Some(model_name) = mode_str.strip_prefix("model:") {
                if let Err(e) = app.switch_model(model_name) {
                    app.add_system_message(format!("Error: {}", e));
                }
            } else if let Some(profile_name) = mode_str.strip_prefix("profile:") {
                if let Err(e) = app.profile_registry.switch(profile_name) {
                    app.add_system_message(format!("❌ {}", e));
                } else {
                    // Apply profile to security engine config
                    let new_config = app.profile_registry.apply_to_config(&app.security_engine.config);
                    app.security_engine = crate::security::SecurityEngine::new(new_config).unwrap_or_else(|e| {
                        app.add_system_message(format!("⚠️ Failed to apply security profile: {}", e));
                        app.security_engine.clone()
                    });
                    app.add_system_message(format!("🔒 {}", app.profile_registry.active_summary()));
                }
            } else {
                app.add_system_message(format!("🔄 Switched to mode: {}", mode_str));
            }
            Ok(true)
        }
        SlashResult::Handled => {
            // Commands that need special TUI-side handling
            // /help, /clear, /context, /stats, /export — handled below
            // /usage — display session token/cost stats
            if let Some(cmd) = input.strip_prefix('/') {
                let name = cmd.split_whitespace().next().unwrap_or(cmd);
                if name == "usage" || name == "cost" || name == "tokens" {
                    let (total_tokens, total_cost) = crate::providers::get_session_usage();
                    app.add_system_message(format!(
                        "📊 Session Usage: {} tokens | ${:.4} estimated",
                        total_tokens, total_cost
                    ));
                    return Ok(true);
                }
                if name == "plugins" || name == "hooks" || name == "extensions" {
                    if let Some(ref registry) = app.plugin_registry {
                        let plugins: Vec<String> = registry.list().iter().map(|p| p.name.clone()).collect();
                        if plugins.is_empty() {
                            app.add_system_message("🔌 No plugins loaded.".to_string());
                        } else {
                            app.add_system_message(format!("🔌 Loaded plugins ({}): {}", plugins.len(), plugins.join(", ")));
                        }
                    } else {
                        app.add_system_message("🔌 Plugin registry not initialized.".to_string());
                    }
                    return Ok(true);
                }
                if name == "profiles" || name == "perms" || name == "security-profiles" {
                    let active = app.profile_registry.active();
                    let mut lines = vec![
                        format!("🔒 Active profile: {}", active),
                        String::new(),
                        "Available profiles:".to_string(),
                    ];
                    for name in app.profile_registry.list() {
                        let marker = if name == active { "▸ " } else { "  " };
                        if let Some(p) = app.profile_registry.get(name) {
                            lines.push(format!("{}{} — {}", marker, name, p.description));
                        }
                    }
                    app.add_system_message(lines.join("\n"));
                    return Ok(true);
                }
                if name == "archive" || name == "hide" || name == "stash" {
                    match app.memory.archive_session(&app.session_id) {
                        Ok(()) => {
                            app.add_system_message("📦 Session archived. It will no longer appear in the active sessions list. Use /archived to see archived sessions.".to_string());
                        }
                        Err(e) => {
                            app.add_system_message(format!("❌ Failed to archive session: {}", e));
                        }
                    }
                    return Ok(true);
                }
                if name.starts_with("unarchive") || name == "unhide" || name == "restore-session" {
                    let args = cmd.splitn(2, ' ').nth(1).unwrap_or("").trim();
                    if args.is_empty() {
                        app.add_system_message("Usage: /unarchive <session-id>".to_string());
                    } else {
                        match app.memory.unarchive_session(args) {
                            Ok(()) => {
                                app.add_system_message(format!("📦 Session {} unarchived. It will now appear in the active sessions list.", args));
                            }
                            Err(e) => {
                                app.add_system_message(format!("❌ Failed to unarchive session: {}", e));
                            }
                        }
                    }
                    return Ok(true);
                }
                if name == "archived" || name == "hidden" || name == "stashed" {
                    match app.memory.get_archived_sessions(50) {
                        Ok(sessions) => {
                            if sessions.is_empty() {
                                app.add_system_message("📭 No archived sessions.".to_string());
                            } else {
                                let mut lines = vec![
                                    format!("📦 Archived Sessions ({}):", sessions.len()),
                                    "─".repeat(50),
                                ];
                                for s in sessions {
                                    let date = s.started_at.format("%Y-%m-%d %H:%M");
                                    lines.push(format!(
                                        "  {} | {} | {} | {}",
                                        s.id, date, s.model, s.task_type
                                    ));
                                }
                                lines.push("".to_string());
                                lines.push("Use /unarchive <session-id> to restore.".to_string());
                                app.add_system_message(lines.join("\n"));
                            }
                        }
                        Err(e) => {
                            app.add_system_message(format!("❌ Failed to list archived sessions: {}", e));
                        }
                    }
                    return Ok(true);
                }
                // /ctx list — show pinned files
                if name == "ctx" || name == "pin" {
                    let args = cmd.splitn(2, ' ').nth(1).unwrap_or("").trim();
                    if args.is_empty() || args == "list" {
                        let lines = app.smart_context.list();
                        app.add_system_message(lines.join("\n"));
                        return Ok(true);
                    }
                }
                // /plugin list — show plugins
                if name == "plugin" || name == "plugins" || name == "hook" || name == "hooks" {
                    let args = cmd.splitn(2, ' ').nth(1).unwrap_or("").trim();
                    if args.is_empty() || args == "list" {
                        if let Some(ref registry) = app.plugin_registry {
                            let plugins: Vec<String> = registry.list().iter().map(|p| format!("{} — {}", p.name, p.description)).collect();
                            if plugins.is_empty() {
                                app.add_system_message("🔌 No plugins loaded. Create one with /plugin create <name>".to_string());
                            } else {
                                let mut lines = vec![format!("🔌 Loaded Plugins ({}):", plugins.len())];
                                for p in plugins {
                                    lines.push(format!("  • {}", p));
                                }
                                app.add_system_message(lines.join("\n"));
                            }
                        } else {
                            app.add_system_message("🔌 Plugin registry not initialized.".to_string());
                        }
                        return Ok(true);
                    }
                }
            }
            Ok(false) // Fall through for now
        }
        SlashResult::Error(e) => {
            app.add_system_message(format!("❌ {}", e));
            Ok(true)
        }
    }
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
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
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

    // Auto-checkpoint before edit operations so user can /undo
    if is_edit_tool(&suggestion.tool_name) && crate::tools::checkpoint::in_git_repo() {
        let cp_name = format!("pre-{}-{}", suggestion.tool_name, chrono::Local::now().format("%H%M%S"));
        match crate::tools::checkpoint::save_checkpoint(&cp_name) {
            Ok(cp) => {
                app.checkpoint_stack.push(cp);
            }
            Err(_) => {
                // Non-fatal — just skip if checkpoint fails (e.g., clean repo)
            }
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
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            });
            app.model_messages.push(Message {
                role: "user".to_string(),
                content: format!("Tool result: {}", sanitized),
                images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
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
                    app.add_assistant_message(follow_content, None);
                }
                Err(e) => app.add_system_message(format!("Follow-up failed: {}", e)),
            }

            // Auto-commit if enabled and tool was an edit
            if app.config.auto_commit && is_edit_tool(&suggestion.tool_name) {
                if let Err(e) = auto_commit_changes(app).await {
                    app.add_system_message(format!("⚠️ Auto-commit failed: {}", e));
                }
            }

            // Auto-run tests if enabled and tool was an edit
            if app.config.auto_run_tests && is_edit_tool(&suggestion.tool_name) {
                if let Err(e) = auto_run_tests(app).await {
                    app.add_system_message(format!("⚠️ Auto-test failed: {}", e));
                }
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
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            });
            follow_messages.push(Message {
                role: "user".to_string(),
                content: format!("Tool result: {}", sanitized),
                images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
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
                    if !follow_content.starts_with("TOOL:") && !follow_content.starts_with("TOOL.") {
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
                                    // Execute chained tool and stream result back through event channel
                                    match executor
                                        .execute_with_timeout_simple(
                                            next_tool.clone(),
                                            next_args.clone(),
                                            30000,
                                        )
                                        .await
                                    {
                                        Ok(result) => {
                                            let _ = tx.send(StreamEvent::ToolResult {
                                                name: next_tool.clone(),
                                                args: next_args.clone(),
                                                result: result.clone(),
                                                success: true,
                                            });
                                            // Build follow-up messages for synthesis
                                            let mut chained_messages = follow_messages.clone();
                                            chained_messages.push(Message {
                                                role: "assistant".to_string(),
                                                content: format!("TOOL:{} {}", next_tool, next_args),
                                                images: None,
                                            tool_call_id: None,
                                            tool_calls: None,
                                            reasoning_content: None,
                                            });
                                            chained_messages.push(Message {
                                                role: "user".to_string(),
                                                content: format!("Tool result: {}", result),
                                                images: None,
                                            tool_call_id: None,
                                            tool_calls: None,
                                            reasoning_content: None,
                                            });
                                            let chained_req = ChatRequest::new(
                                                model.clone(),
                                                chained_messages,
                                                true,
                                            );
                                            match provider.chat_stream(chained_req).await {
                                                Ok((chunks, _metrics)) => {
                                                    let chained_content = chunks.join("");
                                                    let _ = tx.send(StreamEvent::FollowUp(chained_content));
                                                }
                                                Err(e) => {
                                                    let _ = tx.send(StreamEvent::Error(format!("Chained follow-up failed: {}", e)));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx.send(StreamEvent::ToolResult {
                                                name: next_tool,
                                                args: next_args,
                                                result: e.to_string(),
                                                success: false,
                                            });
                                        }
                                    }
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
    if app.mode == AppMode::Splash {
        draw_splash_screen(f);
        return;
    }

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

    if app.mode == AppMode::DiffPreview {
        draw_diff_preview_popup(f, app);
    }

    if app.show_comparison {
        draw_comparison_overlay(f, app);
    }

    // Command palette overlay (drawn last so it's on top)
    command_palette::draw_command_palette(f, &app.command_palette, size);

    // Bookmark manager overlay
    bookmarks::draw_bookmark_manager(f, &app.bookmark_manager, size);
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
    let ctx_pct = (ctx_used * 100)
        .checked_div(app.model_context_length)
        .unwrap_or(0)
        .min(100);
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
    } else if app.sidebar_tab == 4 {
        // Files tab — project file tree
        let file_lines: Vec<Line> = if app.file_tree.is_empty() {
            vec![Line::from(vec![Span::styled("No files scanned", muted_style())])]
        } else {
            app.file_tree.iter().enumerate()
                .skip(app.sidebar_scroll)
                .take(20)
                .map(|(i, entry)| {
                    let is_selected = i == app.file_tree_selected;
                    let style = if is_selected {
                        highlight_style().add_modifier(Modifier::BOLD)
                    } else {
                        text_style()
                    };
                    Line::from(vec![Span::styled(entry.clone(), style)])
                })
                .collect()
        };
        (" Files ".to_string(), file_lines)
    } else if app.sidebar_tab == 2 {
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
    } else {
        // Inspector tab — per-agent raw output
        let inspector_lines: Vec<Line> = if !app.agent_streams.is_empty() {
            let mut lines = vec![
                Line::from(vec![Span::styled("Agent Inspector", title_style())]),
                Line::from(vec![]),
            ];
            for (_, state) in app.agent_streams.iter().skip(app.sidebar_scroll).take(8) {
                let role_color = match state.role.as_str() {
                    "Architect" => current_theme().accent,
                    "Implementer" => current_theme().success,
                    "Reviewer" => current_theme().highlight,
                    "Tester" => current_theme().error,
                    _ => current_theme().accent,
                };
                let role_style = Style::default().fg(role_color).add_modifier(Modifier::BOLD);
                let status = if state.is_streaming { "🟡 streaming" } else { "⏹ done" };
                lines.push(Line::from(vec![
                    Span::styled(format!("🐝 {} ", state.agent_name), role_style),
                    Span::styled(format!("({}) {}", state.role, status), muted_style()),
                ]));
                // Show truncated content preview with code detection
                let preview = &state.content[..state.content.len().min(120)];
                let has_code = preview.contains("```") || preview.contains("fn ") || preview.contains("def ");
                if has_code {
                    lines.push(Line::from(vec![
                        Span::styled("  📄 ".to_string(), muted_style()),
                        Span::styled(preview.replace('\n', " "), muted_style()),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {}", preview.replace('\n', " ")), muted_style()),
                    ]));
                }
                if !state.tool_results.is_empty() {
                    let expanded = app.agent_tool_expanded.contains(&state.agent_id);
                    let expand_icon = if expanded { "▼" } else { "▶" };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {} 🔧 {} tools", expand_icon, state.tool_results.len()), tool_style()),
                    ]));
                    if expanded {
                        for (tool_name, result, success) in &state.tool_results {
                            let icon = if *success { "✅" } else { "❌" };
                            lines.push(Line::from(vec![
                                Span::styled(format!("    {} {}", icon, tool_name), muted_style()),
                            ]));
                            for res_line in result.lines().take(4) {
                                lines.push(Line::from(vec![
                                    Span::styled(format!("      {}", res_line.chars().take(50).collect::<String>()), muted_style()),
                                ]));
                            }
                        }
                    }
                }
                lines.push(Line::from(vec![]));
            }
            lines
        } else {
            vec![
                Line::from(vec![Span::styled("No agent data", muted_style())]),
                Line::from(vec![Span::styled("Start a swarm to inspect agents", muted_style())]),
            ]
        };
        (" Inspector ".to_string(), inspector_lines)
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

    // Performance — per-session metrics (streaming) or swarm stats
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
    } else if app.swarm_running {
        // Show swarm stats when no streaming perf data
        let active = app.swarm_agents.iter()
            .filter(|a| matches!(a.status, crate::swarm::AgentStatus::Working { .. }))
            .count();
        let completed = app.swarm_agents.iter()
            .filter(|a| matches!(a.status, crate::swarm::AgentStatus::Completed { .. }))
            .count();
        let errors = app.swarm_agents.iter()
            .filter(|a| matches!(a.status, crate::swarm::AgentStatus::Error { .. }))
            .count();
        vec![
            Line::from(vec![
                Span::styled("Swarm active", accent_style()),
            ]),
            Line::from(vec![
                Span::styled("Working: ", muted_style()),
                Span::styled(format!("{}", active), text_style()),
                Span::styled(" | Done: ", muted_style()),
                Span::styled(format!("{}", completed), text_style()),
                Span::styled(" | Err: ", muted_style()),
                Span::styled(format!("{}", errors), if errors > 0 { error_style() } else { text_style() }),
            ]),
            Line::from(vec![
                Span::styled("Cycles: ", muted_style()),
                Span::styled(
                    format!("{}", app.swarm_agents.iter().map(|a| a.cycles_completed).sum::<usize>()),
                    text_style()
                ),
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

        // Image attachment indicator with rich metadata
        if let Some(ref images) = msg.images {
            for img in images {
                let info = image_display::extract_image_info(img);
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {}", info.format_indicator()),
                        muted_style().add_modifier(Modifier::ITALIC),
                    ),
                ]));
                // Show ASCII placeholder for the image
                for placeholder_line in info.ascii_placeholder() {
                    lines.push(Line::from(vec![
                        Span::styled(placeholder_line, muted_style()),
                    ]));
                }
            }
        }

        // Render content with syntax highlighting for assistant messages
        if msg.role == "assistant" {
            // Render persistent reasoning first, if present
            if let Some(ref reasoning) = msg.reasoning
                && !reasoning.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("  💭 ", reasoning_style()),
                        Span::styled("Thinking", reasoning_style().add_modifier(Modifier::BOLD | Modifier::ITALIC)),
                    ]));
                    for line in reasoning.lines() {
                        lines.push(Line::from(vec![
                            Span::styled(format!("     {}", line), reasoning_style().add_modifier(Modifier::ITALIC)),
                        ]));
                    }
                    lines.push(Line::from(""));
                }

            // Render assistant messages with syntax highlighting + inline markdown
            let highlighted = syntax_highlight::extract_and_highlight(&msg.content);
            for (is_code, block_lines) in highlighted {
                if is_code {
                    lines.push(Line::from(vec![
                        Span::styled("┌─ code ──────────────────────────────", muted_style()),
                    ]));
                    for hl_line in block_lines {
                        lines.push(hl_line);
                    }
                    lines.push(Line::from(vec![
                        Span::styled("└─────────────────────────────────────", muted_style()),
                    ]));
                } else {
                    for hl_line in block_lines {
                        // Apply inline markdown rendering to plain text blocks
                        let rendered = syntax_highlight::render_markdown_line(&hl_line.to_string());
                        lines.push(rendered);
                    }
                }
            }

        } else {
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
        // Animated spinner based on frame counter
        const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let spinner = SPINNER_CHARS[app.spinner_frame % SPINNER_CHARS.len()];
        let elapsed = app.stream_start_time.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        let elapsed_str = if elapsed > 0 {
            format!(" [{}s]", elapsed)
        } else {
            String::new()
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{} ", agent_emoji), shark_style()),
            Span::styled(agent_name, shark_style().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {} thinking{}", spinner, elapsed_str), accent_style()),
        ]));

        // Show reasoning/thinking content in real-time with syntax highlighting
        if !app.reasoning_content.is_empty() {
            // Use the same syntax highlighter so code in reasoning gets highlighted
            let highlighted = syntax_highlight::extract_and_highlight(&app.reasoning_content);
            for (is_code, block_lines) in highlighted {
                if is_code {
                    lines.push(Line::from(vec![
                        Span::styled("┌─ code ──────────────────────────────", muted_style()),
                    ]));
                    for hl_line in block_lines {
                        lines.push(hl_line);
                    }
                    lines.push(Line::from(vec![
                        Span::styled("└─────────────────────────────────────", muted_style()),
                    ]));
                } else {
                    for hl_line in block_lines {
                        lines.push(hl_line);
                    }
                }
            }
        }

        // Show actual streaming content with syntax highlighting
        let highlighted = syntax_highlight::extract_and_highlight(&app.streaming_content);
        for (is_code, block_lines) in highlighted {
            if is_code {
                lines.push(Line::from(vec![
                    Span::styled("┌─ code ──────────────────────────────", muted_style()),
                ]));
                for hl_line in block_lines {
                    lines.push(hl_line);
                }
                lines.push(Line::from(vec![
                    Span::styled("└─────────────────────────────────────", muted_style()),
                ]));
            } else {
                for hl_line in block_lines {
                    lines.push(hl_line);
                }
            }
        }
        lines.push(Line::from(vec![Span::styled("▌", accent_style())]));
    }

    // ── Swarm Agent Streaming ──────────────────────────────────────────────
    for (_, state) in app.agent_streams.iter() {
        if state.is_streaming || !state.content.is_empty() {
            let role_color = match state.role.as_str() {
                "Architect" => current_theme().accent,
                "Implementer" => current_theme().success,
                "Reviewer" => current_theme().highlight,
                "Tester" => current_theme().error,
                "DevOps" => current_theme().accent_secondary,
                "Security" => current_theme().error,
                "Documentation" => current_theme().muted,
                "Project Manager" => current_theme().title,
                _ => current_theme().accent,
            };
            let role_style = Style::default().fg(role_color).add_modifier(Modifier::BOLD);

            lines.push(Line::from(vec![
                Span::styled("🐝 ", role_style),
                Span::styled(format!("{} — {}", state.agent_name, state.role), role_style),
            ]));

            // Use syntax highlighting for code blocks in agent content
            let highlighted = syntax_highlight::extract_and_highlight(&state.content);
            for (is_code, block_lines) in highlighted {
                if is_code {
                    // Add a subtle code block border
                    lines.push(Line::from(vec![
                        Span::styled("┌─ code ──────────────────────────────", muted_style()),
                    ]));
                    for hl_line in block_lines {
                        lines.push(hl_line);
                    }
                    lines.push(Line::from(vec![
                        Span::styled("└─────────────────────────────────────", muted_style()),
                    ]));
                } else {
                    for hl_line in block_lines {
                        lines.push(hl_line);
                    }
                }
            }

            if state.is_streaming {
                lines.push(Line::from(vec![Span::styled("▌", role_style)]));
            }

            lines.push(Line::from(""));
        }
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
    // Split into status line + input area
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    // ── Status line ─────────────────────────────────────────────────────────
    let mut status_spans = vec![];

    // YOLO mode indicator
    if app.yolo_mode {
        status_spans.push(Span::styled("🤘YOLO ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
    }

    // Checkpoint depth
    let cp_depth = app.checkpoint_stack.undo_len();
    if cp_depth > 0 {
        status_spans.push(Span::styled(
            format!("💾{} ", cp_depth),
            Style::default().fg(Color::Cyan),
        ));
    }

    // Session cost
    let (total_tokens, total_cost) = crate::providers::get_session_usage();
    if total_tokens > 0 {
        status_spans.push(Span::styled(
            format!("💰${:.4} ", total_cost),
            Style::default().fg(Color::Yellow),
        ));
    }

    // Model
    status_spans.push(Span::styled(
        format!("🤖{} ", app.model),
        Style::default().fg(Color::Magenta),
    ));

    // Token count for this session
    if app.tokens_used > 0 {
        status_spans.push(Span::styled(
            format!("📊{}t ", app.tokens_used),
            Style::default().fg(Color::Green),
        ));
    }

    // Streaming indicator
    if app.is_streaming {
        const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let spinner = SPINNER_CHARS[app.spinner_frame % SPINNER_CHARS.len()];
        status_spans.push(Span::styled(
            format!("{} ", spinner),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    }

    let status_line = Paragraph::new(Line::from(status_spans))
        .style(Style::default().bg(Color::Black));
    f.render_widget(status_line, layout[0]);

    // ── Input block ─────────────────────────────────────────────────────────
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

    let inner = input_block.inner(layout[1]);
    f.render_widget(input_block, layout[1]);

    let input_text = if app.input.is_empty() {
        if app.mode == AppMode::ToolApproval {
            "Tool suggestion pending. Press 'y' to execute, 'n' to skip."
        } else if app.is_streaming {
            const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let spinner = SPINNER_CHARS[app.spinner_frame % SPINNER_CHARS.len()];
            let elapsed = app.stream_start_time.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            if elapsed > 0 {
                &format!("{} Streaming response... [{}s]", spinner, elapsed)
            } else {
                &format!("{} Streaming response...", spinner)
            }
        } else if app.swarm_running {
            "🐝 Agents working..."
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
    let explicit_lines = app.input.matches('\n').count() + 1;
    let wrapped_lines = text.div_ceil(available_width); // ceil division
    let lines = explicit_lines.max(wrapped_lines);
    let lines = lines.max(1);
    // Cap at 8 lines so it doesn't eat the whole chat area
    let capped = lines.min(8);
    (capped as u16) + 2 // +2 for borders
}

/// Compute the actual screen (x, y) for the cursor given a text buffer,
/// a cursor byte position, and the available wrap width.
/// This uses word-wrapping logic to match Paragraph::wrap behavior.
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

    let pos = cursor_pos.min(text.len());
    // Ensure we don't slice inside a multi-byte UTF-8 character
    let safe_pos = text.ceil_char_boundary(pos);
    let before_cursor = &text[..safe_pos];
    let mut col: usize = 0;
    let mut row: u16 = 0;
    let mut word_start_col: usize = 0;
    let mut word_width: usize = 0;
    let mut in_word = false;

    // Process text character by character, tracking word boundaries
    for ch in before_cursor.chars() {
        if ch == '\n' {
            row += 1;
            col = 0;
            word_start_col = 0;
            word_width = 0;
            in_word = false;
        } else if ch.is_whitespace() {
            // End of word — commit it
            col = word_start_col + word_width;
            in_word = false;
            let space_width = ch.width().unwrap_or(1);
            if col + space_width > wrap_width {
                // Space goes past wrap boundary — wrap to next line
                // The space itself is consumed by the line break (not shown)
                row += 1;
                col = 0;
            } else {
                col += space_width;
            }
            word_start_col = col;
            word_width = 0;
        } else {
            // In a word
            if !in_word {
                word_start_col = col;
                word_width = 0;
                in_word = true;
            }
            let ch_width = ch.width().unwrap_or(1);
            word_width += ch_width;
            
            // Check if word needs to wrap
            if word_start_col + word_width > wrap_width && word_width > wrap_width {
                // Word is longer than line — force break at char boundary
                row += 1;
                word_start_col = 0;
                word_width = ch_width;
            } else if word_start_col + word_width > wrap_width {
                // Word doesn't fit — wrap to next line
                row += 1;
                word_start_col = 0;
                // word_width stays the same
            }
        }
    }
    
    // Final position is at the end of the last word
    col = word_start_col + word_width;

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

/// Draw the diff preview popup (80% x 70% popup with scrollable diff).
fn draw_diff_preview_popup(f: &mut Frame, app: &App) {
    let area = f.area();
    let popup_area = centered_rect(80, 70, area);

    let clear = Clear;
    f.render_widget(clear, popup_area);

    let block = Block::default()
        .title(" Diff Preview ")
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(focused_border_style())
        .style(bg_style());

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("📝 Proposed file edit:", highlight_style()),
        ]),
        Line::from(""),
    ];

    if let Some(ref diff) = app.pending_diff {
        for diff_line in diff.lines().skip(app.diff_scroll) {
            let styled_line = if diff_line.starts_with('+') {
                Line::from(vec![Span::styled(diff_line, Style::default().fg(ratatui::style::Color::Green))])
            } else if diff_line.starts_with('-') {
                Line::from(vec![Span::styled(diff_line, Style::default().fg(ratatui::style::Color::Red))])
            } else if diff_line.starts_with("@@") {
                Line::from(vec![Span::styled(diff_line, Style::default().fg(ratatui::style::Color::Cyan))])
            } else {
                Line::from(vec![Span::styled(diff_line, text_style())])
            };
            lines.push(styled_line);
        }
    } else {
        lines.push(Line::from("No diff available."));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Press ", text_style()),
        Span::styled("y", accent_style()),
        Span::styled(" to approve diff, ", text_style()),
        Span::styled("n", error_style()),
        Span::styled(" to skip", text_style()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("↑/↓ or PgUp/PgDn to scroll", muted_style()),
    ]));

    let paragraph = Paragraph::new(Text::from(lines))
        .style(bg_style())
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
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
fn draw_chat_header(_f: &mut Frame, _area: Rect) {
    // Removed — welcome message is now in chat history
}

/// Full-screen splash screen — DOS title screen aesthetic.
/// Shows the OpenShark wordmark, fin, waves, and tagline.
/// Press any key to dismiss and enter the chat TUI.
fn draw_splash_screen(f: &mut Frame) {
    let area = f.area();

    // Solid background fill
    let bg = Block::default().style(bg_style());
    f.render_widget(bg, area);

    let banner_text = ascii_art::welcome_banner(area.width as usize);
    let banner_lines: Vec<Line> = banner_text
        .lines()
        .map(|line| {
            // Colorize different parts of the banner
            if line.contains('▪') {
                // Wordmark / fin — purple/pink
                Line::from(vec![Span::styled(
                    line,
                    Style::default()
                        .fg(current_theme().accent_secondary)
                        .add_modifier(Modifier::BOLD),
                )])
            } else if line.contains("Fast. Precise. Hungry.") {
                // Tagline — hot pink
                Line::from(vec![Span::styled(
                    line,
                    Style::default()
                        .fg(current_theme().accent_secondary)
                        .add_modifier(Modifier::BOLD),
                )])
            } else if line.contains('≈') {
                // Waves — cyan/blue tones
                Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(current_theme().accent),
                )])
            } else {
                Line::from(vec![Span::styled(line, text_style())])
            }
        })
        .collect();

    let banner = Paragraph::new(Text::from(banner_lines))
        .alignment(Alignment::Left)
        .style(bg_style());

    // Center vertically: calculate offset to place banner in middle of screen
    let banner_height = banner_text.lines().count() as u16;
    let vertical_offset = (area.height.saturating_sub(banner_height)) / 2;
    let banner_area = Rect {
        x: area.x,
        y: area.y + vertical_offset,
        width: area.width,
        height: banner_height.min(area.height),
    };

    f.render_widget(banner, banner_area);

    // "Press any key" prompt at bottom
    let prompt = Paragraph::new(Text::from(vec![Line::from(vec![Span::styled(
        "Press any key to start",
        Style::default()
            .fg(current_theme().muted)
            .add_modifier(Modifier::ITALIC),
    )])]))
        .alignment(Alignment::Center)
        .style(bg_style());

    let prompt_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(3),
        width: area.width,
        height: 1,
    };
    f.render_widget(prompt, prompt_area);
}

/// Split content into thinking/reasoning and regular content.
/// Handles <think>...</think> blocks from Kimi models.
fn split_thinking_content(content: &str) -> (String, String) {
    let mut thinking = String::new();
    let mut regular = String::new();
    let mut in_think = false;
    let mut think_buffer = String::new();
    let mut regular_buffer = String::new();

    // Simple state machine to parse think tags
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Check for <think> or </think>
            let mut tag = String::new();
            tag.push(ch);
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '>' {
                    tag.push(chars.next().unwrap());
                    break;
                } else {
                    tag.push(chars.next().unwrap());
                }
            }

            if tag == "<think>" {
                // Flush regular buffer
                if !regular_buffer.is_empty() {
                    regular.push_str(&regular_buffer);
                    regular_buffer.clear();
                }
                in_think = true;
            } else if tag == "</think>" {
                // Flush think buffer
                if !think_buffer.is_empty() {
                    if !thinking.is_empty() {
                        thinking.push('\n');
                    }
                    thinking.push_str(&think_buffer);
                    think_buffer.clear();
                }
                in_think = false;
            } else {
                // Not a think tag, treat as regular content
                if in_think {
                    think_buffer.push_str(&tag);
                } else {
                    regular_buffer.push_str(&tag);
                }
            }
        } else {
            if in_think {
                think_buffer.push(ch);
            } else {
                regular_buffer.push(ch);
            }
        }
    }

    // Flush remaining buffers
    if !think_buffer.is_empty() {
        if !thinking.is_empty() {
            thinking.push('\n');
        }
        thinking.push_str(&think_buffer);
    }
    if !regular_buffer.is_empty() {
        regular.push_str(&regular_buffer);
    }

    (thinking, regular)
}

/// Emergency truncate: remove oldest non-system messages until estimated tokens
/// are below `target_tokens`. Preserves system prompt and most recent messages.
fn emergency_truncate_messages(
    messages: &mut Vec<crate::providers::Message>,
    target_tokens: usize,
) {
    loop {
        let estimated = crate::memory::compression::estimate_tokens(messages);
        if estimated <= target_tokens || messages.len() <= 2 {
            break;
        }
        // Find oldest non-system message to remove
        let remove_idx = messages
            .iter()
            .enumerate()
            .skip(1) // Never remove index 0 (system prompt)
            .find(|(_, m)| m.role != "system")
            .map(|(i, _)| i);
        if let Some(idx) = remove_idx {
            messages.remove(idx);
        } else {
            break;
        }
    }
}

/// Generate an AI commit message from the current diff.
async fn generate_commit_message(app: &mut App) -> Result<String> {
    let git_tool = crate::tools::GitTool;
    let diff = match git_tool.execute("diff --staged") {
        Ok(d) => d,
        Err(e) => return Err(anyhow::anyhow!("Failed to get staged diff: {}", e)),
    };

    if diff.trim().is_empty() {
        return Ok("chore: no changes".to_string());
    }

    let prompt = format!(
        "Generate a concise conventional commit message for this diff. \
         Use format: type(scope): description. Types: feat, fix, docs, style, refactor, test, chore. \
         Max 72 chars for first line. Be specific about what changed.\n\n```diff\n{}\n```",
        diff.chars().take(4000).collect::<String>()
    );

    let request = ChatRequest {
        model: app.model.clone(),
        messages: vec![
            Message { role: "system".to_string(), content: "You generate concise conventional commit messages.".to_string(), images: None, tool_call_id: None, tool_calls: None, reasoning_content: None },
            Message { role: "user".to_string(), content: prompt, images: None, tool_call_id: None, tool_calls: None, reasoning_content: None },
        ],
        stream: false,
        temperature: Some(0.3),
        max_tokens: Some(100),
        tools: None,
    };

    let response = match app.provider.chat(request).await {
        Ok(r) => r.choices.first().map(|c| c.message.content.clone()).unwrap_or_else(|| "chore: update".to_string()),
        Err(e) => return Err(anyhow::anyhow!("LLM error: {}", e)),
    };

    let msg = response.lines().next().unwrap_or("chore: update").trim().to_string();
    let msg = msg.trim_start_matches("Commit message:").trim().to_string();
    let msg = msg.trim_start_matches('"').trim_end_matches('"').to_string();

    if msg.is_empty() {
        Ok("chore: update".to_string())
    } else {
        Ok(msg)
    }
}

/// Generate a diff preview for an edit tool suggestion.
/// Returns Some(diff) if the suggestion is for write/replace/patch and a diff can be generated.
fn generate_edit_diff(args: &str) -> Option<String> {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return None;
    }

    let cmd = parts[0];
    let rest = parts[1];

    match cmd {
        "write" => {
            let wparts: Vec<&str> = rest.splitn(2, ' ').collect();
            if wparts.len() < 2 {
                return None;
            }
            let path = wparts[0];
            let content = wparts[1];
            crate::diff::preview_write(path, content).ok()
        }
        "replace" => {
            let delimiter = " ||| ";
            let rp: Vec<&str> = rest.splitn(2, delimiter).collect();
            if rp.len() < 2 {
                return None;
            }
            let pp: Vec<&str> = rp[0].splitn(2, ' ').collect();
            if pp.len() < 2 {
                return None;
            }
            let path = pp[0];
            let old_str = pp[1];
            let new_str = rp[1];
            crate::diff::preview_replace(path, old_str, new_str).ok()
        }
        "patch" => {
            let delimiter = " ||| ";
            let pp: Vec<&str> = rest.splitn(2, delimiter).collect();
            if pp.len() < 2 {
                return None;
            }
            let ppp: Vec<&str> = pp[0].splitn(2, ' ').collect();
            if ppp.len() < 2 {
                return None;
            }
            let path = ppp[0];
            let old_lines = ppp[1];
            let new_lines = pp[1];
            crate::diff::preview_patch(path, old_lines, new_lines).ok()
        }
        _ => None,
    }
}

/// Check if a tool name is an edit operation that should trigger auto-commit.
fn is_edit_tool(tool_name: &str) -> bool {
    matches!(tool_name, "edit" | "write" | "patch" | "replace" | "refactor")
}

/// Auto-commit changes after a successful edit.
async fn auto_commit_changes(app: &mut App) -> Result<()> {
    let git_tool = crate::tools::GitTool;
    // Check if there are changes to commit
    match git_tool.execute("status --short") {
        Ok(status) => {
            if status.trim().is_empty() {
                return Ok(()); // Nothing to commit
            }
        }
        Err(_) => return Ok(()), // Not a git repo or git not available
    }

    // Stage all changes
    git_tool.execute("add .")?;

    // Generate commit message
    let commit_msg = match generate_commit_message(app).await {
        Ok(msg) => msg,
        Err(_) => {
            format!("openshark: auto-commit at {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
        }
    };

    // Commit
    match git_tool.execute(&format!("commit {}", commit_msg)) {
        Ok(output) => {
            app.add_system_message(format!("🤖 Auto-committed: {}\n```\n{}\n```", commit_msg, output.trim()));
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Git commit failed: {}", e));
        }
    }

    Ok(())
}

/// Auto-run tests after a successful edit.
async fn auto_run_tests(app: &mut App) -> Result<()> {
    let test_tool = crate::tools::test_runner::TestTool;
    let result = match app.config.test_command.as_ref() {
        Some(cmd) => {
            // Run custom test command
            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .output()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to run test command: {} — {}", cmd, e))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("Test command: {}\n\nstdout:\n{}\n\nstderr:\n{}\n\nExit code: {:?}",
                cmd, stdout, stderr, output.status.code())
        }
        None => {
            // Auto-detect and run
            test_tool.execute("run .")?
        }
    };

    let status = if result.contains("FAILED") || result.contains("error[") {
        "❌ FAILED"
    } else {
        "✅ PASSED"
    };

    app.add_system_message(format!("🧪 Auto-test results: {}\n```\n{}\n```", status, result));
    Ok(())
}

/// Create a git worktree for isolated background task execution.
/// Returns the path to the new worktree directory.
async fn create_worktree(project_path: &str, task: &str) -> anyhow::Result<String> {
    // Sanitize task name for directory
    let sanitized: String = task
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .take(40)
        .collect();
    let branch_name = format!("openshark-headless-{}", sanitized);
    let worktree_path = format!("{}/.openshark-worktrees/{}", project_path, branch_name);

    // Ensure the worktrees directory exists
    let _ = tokio::fs::create_dir_all(format!("{}/.openshark-worktrees", project_path)).await;

    // Create worktree from current HEAD
    let output = tokio::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "-B",
            &branch_name,
            &worktree_path,
            "HEAD",
        ])
        .current_dir(project_path)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("git worktree add failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("git worktree add failed: {}", stderr));
    }

    tracing::info!("[worktree] Created {} at {}", branch_name, worktree_path);
    Ok(worktree_path)
}

/// Remove a git worktree and clean up.
async fn remove_worktree(project_path: &str, worktree_path: &str) -> anyhow::Result<()> {
    let output = tokio::process::Command::new("git")
        .args(["worktree", "remove", "--force", worktree_path])
        .current_dir(project_path)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("git worktree remove failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("[worktree] Remove warning: {}", stderr);
    }

    // Prune orphaned worktrees
    let _ = tokio::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(project_path)
        .output()
        .await;

    tracing::info!("[worktree] Removed {}", worktree_path);
    Ok(())
}
