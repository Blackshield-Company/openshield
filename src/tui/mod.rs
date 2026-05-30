use anyhow::Result;
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
use uuid::Uuid;

mod theme;
use theme::*;

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
    /// Streaming finished (success or error).
    Done,
}

/// A single message in the chat history.
#[derive(Debug, Clone)]
struct ChatMessage {
    role: String,
    content: String,
    #[allow(dead_code)]
    timestamp: chrono::DateTime<Utc>,
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
    /// Security engine for guardrails.
    security_engine: crate::security::SecurityEngine,
    /// Autonomous mode — temporarily elevate risk tolerance for full-send coding.
    autonomous_mode: bool,
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
        let system_msg = Message {
            role: "system".to_string(),
            content: format!(
                "{}\n\nYou have access to tools:\n{}\n\
                 When you need to use a tool, respond with: TOOL:tool_name args\n\
                 Be concise and direct. Don't overthink.",
                soul.system_prompt(),
                get_tools()
                    .iter()
                    .map(|t| format!("- {}: {}", t.name(), t.description()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
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
            config,
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
            security_engine,
            autonomous_mode: false,
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
        let models: Vec<String> = self.config.providers.iter()
            .flat_map(|(provider_name, provider)| {
                provider.models.iter().map(move |m| {
                    format!("{} ({})", m.name, provider_name)
                })
            })
            .collect();

        let mut msg = String::from("Available models:\n");
        for (i, model) in models.iter().enumerate() {
            let indicator = if self.model == model.split(" (").next().unwrap_or("") {
                "●"
            } else {
                "○"
            };
            msg.push_str(&format!("  {} {} (type /model {} to switch)\n", indicator, model, i));
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
        let msg = ChatMessage {
            role: "user".to_string(),
            content: content.clone(),
            timestamp: Utc::now(),
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

        self.model_messages.push(Message {
            role: "user".to_string(),
            content,
        });

        self.tokens_used += token_count;
    }

    fn add_assistant_message(&mut self, content: String) {
        let token_count = content.split_whitespace().count() as u64;
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: content.clone(),
            timestamp: Utc::now(),
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
        });

        self.tokens_used += token_count;
    }

    /// Add a system/tool message to the chat.
    fn add_system_message(&mut self, content: String) {
        let msg = ChatMessage {
            role: "system".to_string(),
            content: content.clone(),
            timestamp: Utc::now(),
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

                if content.starts_with("TOOL:") {
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
                        });
                    }
                } else {
                    self.add_assistant_message(content.clone());
                    if let Some(suggestion) = detect_high_confidence_suggestion(&content) {
                        self.pending_suggestion = Some(suggestion);
                        self.mode = AppMode::ToolApproval;
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

                self.model_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool result: {}", result),
                });
            }
            StreamEvent::FollowUp(content) => {
                self.add_assistant_message(content);
            }
            StreamEvent::MultiModelResponse { name, content, metrics } => {
                if !content.is_empty() {
                    self.add_system_message(format!(
                        "[{}] {}ms | {} tokens\n{}",
                        name,
                        metrics.total_latency_ms,
                        metrics.tokens_generated,
                        &content[..content.len().min(500)]
                    ));
                }
            }
            StreamEvent::SetPendingSuggestion(suggestion) => {
                self.pending_suggestion = Some(suggestion);
                self.mode = AppMode::ToolApproval;
            }
            StreamEvent::Error(msg) => {
                self.is_streaming = false;
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
}

pub async fn run(config: Config) -> Result<()> {
    // Initialize theme from config
    if let Some(theme) = crate::tui::theme::Theme::by_name(&config.theme) {
        crate::tui::theme::set_theme(theme);
    }

    let mut terminal = ratatui::init();
    terminal.clear()?;

    let mut app = App::new(config.clone())?;

    // Inject welcome message using the agent's configured identity
    let welcome = format!(
        "\n{} {}\n{}\n\n{}",
        config.agent.emoji,
        config.agent.display_name,
        config.agent.tagline,
        config.agent.greeting
    );
    app.add_system_message(welcome);
    let mut last_tick = Instant::now();

    let result = run_app(&mut terminal, &mut app, &mut last_tick).await;

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

        if app.should_exit {
            break;
        }
    }

    Ok(())
}

async fn handle_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    match app.mode {
        AppMode::ToolApproval => {
            match key.code {
                KeyCode::Char('y') => {
                    if let Some(suggestion) = app.pending_suggestion.take() {
                        app.mode = AppMode::Normal;
                        let _ = execute_tool_suggestion(app, &suggestion).await;
                    }
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    app.pending_suggestion = None;
                    app.mode = AppMode::Normal;
                    app.add_system_message("⏭ Skipped tool suggestion.".to_string());
                }
                _ => {}
            }
            return Ok(false);
        }
        _ => {}
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
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
        KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::CONTROL) => {
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
            app.scroll_up(3);
        }
        KeyCode::Down => {
            app.scroll_down(3);
        }
        KeyCode::PageUp => {
            app.scroll_up(10);
        }
        KeyCode::PageDown => {
            app.scroll_down(10);
        }
        KeyCode::Esc => {
            return Ok(true);
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
            "🦈 OpenShark Commands\n\
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
            Branch commands:\n\
            • /branch <name>    — Create new branch\n\
            • /branches         — List branches\n\
            • /switch <index>   — Switch to branch\n\
            \n\
            Keybindings:\n\
            • Ctrl+C            — Copy / Quit (double-tap)\n\
            • Ctrl+L            — Clear chat\n\
            • Ctrl+B            — Toggle sidebar\n\
            • Ctrl+M            — Model selector\n\
            • Ctrl+A            — Toggle autonomous mode\n\
            • Ctrl+T            — Cycle theme\n\
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

    let provider = app.provider.clone();
    let model = app.model.clone();
    let model_config = app.model_config.clone();
    let model_messages = app.model_messages.clone();
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
            if full_content.starts_with("TOOL:") {
                let rest = &full_content[5..];
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if !parts.is_empty() {
                    let tool_name = parts[0].trim().to_string();
                    let args = parts.get(1).unwrap_or(&"").trim().to_string();

                    // SECURITY GATE in background task
                    match security_engine.check_tool_call(&tool_name, &args) {
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

                    let executor = AsyncToolExecutor::new();
                    match executor
                        .execute_with_timeout_simple(tool_name.clone(), args.clone(), 30000)
                        .await
                    {
                        Ok(result) => {
                            let sanitized = security_engine.sanitize_output(&tool_name, &result);
                            let _ = tx.send(StreamEvent::ToolResult {
                                name: tool_name.clone(),
                                args: args.clone(),
                                result: sanitized.clone(),
                                success: true,
                            });

                            // Follow-up request
                            let mut follow_messages = model_messages.clone();
                            follow_messages.push(Message {
                                role: "assistant".to_string(),
                                content: format!("TOOL:{} {}", tool_name, args),
                            });
                            follow_messages.push(Message {
                                role: "user".to_string(),
                                content: format!("Tool result: {}", sanitized),
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
                                }
                                Err(e) => {
                                    let _ = tx.send(StreamEvent::Error(format!("Follow-up failed: {}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(StreamEvent::ToolResult {
                                name: tool_name,
                                args,
                                result: e.to_string(),
                                success: false,
                            });
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
            });
            app.model_messages.push(Message {
                role: "user".to_string(),
                content: format!("Tool result: {}", sanitized),
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
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(main_layout[1]);

    draw_chat_area(f, app, chat_layout[0]);
    draw_input_bar(f, app, chat_layout[1]);

    if app.mode == AppMode::ToolApproval {
        draw_tool_approval_popup(f, app);
    }
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    // Single outer border for the whole sidebar — no nested boxes
    let sidebar_block = Block::default()
        .title(format!(" {} ", app.config.agent.emoji))
        .title_style(title_style())
        .borders(Borders::ALL)
        .border_style(border_style())
        .style(bg_style());

    let inner = sidebar_block.inner(area);
    f.render_widget(sidebar_block, area);

    // Compact vertical layout: header → session → shortcuts → tools → perf
    let sidebar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Compact logo + tagline
            Constraint::Length(7),  // Session info (5 lines + padding)
            Constraint::Length(7),  // Shortcuts (5 lines + padding)
            Constraint::Length(6),  // Tools (up to 5)
            Constraint::Min(3),     // Performance (flexible)
        ])
        .split(inner);

    // Compact header: agent identity + version
    let agent_emoji = app.config.agent.emoji.clone();
    let agent_name = app.config.agent.display_name.clone();
    let header_lines = vec![
        Line::from(vec![
            Span::styled(format!("{} ", agent_emoji), shark_style()),
            Span::styled(agent_name, highlight_style()),
            Span::styled(" v0.2.0", muted_style()),
        ]),
        Line::from(vec![
            Span::styled(app.config.agent.tagline.clone(), muted_style()),
        ]),
    ];
    let header = Paragraph::new(Text::from(header_lines))
        .alignment(Alignment::Center)
        .style(bg_style());
    f.render_widget(header, sidebar_layout[0]);

    // Session info — no inner border, just styled text with section header
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
        Line::from(vec![Span::styled("Ctrl+C  ", accent_style()), Span::styled("Copy / Quit", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+L  ", accent_style()), Span::styled("Clear chat", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+B  ", accent_style()), Span::styled("Toggle sidebar", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+M  ", accent_style()), Span::styled("Model selector", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+A  ", accent_style()), Span::styled("Autonomous mode", muted_style())]),
        Line::from(vec![Span::styled("Ctrl+T  ", accent_style()), Span::styled("Cycle theme", muted_style())]),
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

    // Tools — just names, descriptions truncated
    let all_tools = get_tools();
    let tools: Vec<Line> = all_tools
        .iter()
        .take(5)
        .map(|t| {
            let desc = t.description();
            let desc_short = &desc[..desc.len().min(18)];
            Line::from(vec![
                Span::styled(format!("{:<8}", t.name()), tool_style()),
                Span::styled(desc_short.to_string(), muted_style()),
            ])
        })
        .collect();
    let tools_para = Paragraph::new(Text::from(tools))
        .block(
            Block::default()
                .title(" Tools ")
                .title_style(title_style())
                .borders(Borders::TOP)
                .border_style(border_style()),
        )
        .style(bg_style());
    f.render_widget(tools_para, sidebar_layout[3]);

    // Performance — compact, no border
    let perf_lines = match app.memory.get_performance_summary() {
        Ok(summary) if summary.total_requests > 0 => vec![
            Line::from(vec![
                Span::styled("First token: ", muted_style()),
                Span::styled(format!("{}ms", summary.avg_first_token_ms), text_style()),
            ]),
            Line::from(vec![
                Span::styled("Total latency: ", muted_style()),
                Span::styled(format!("{}ms", summary.avg_total_latency_ms), text_style()),
            ]),
            Line::from(vec![
                Span::styled("Tool exec: ", muted_style()),
                Span::styled(format!("{}ms", summary.avg_tool_execution_ms), text_style()),
            ]),
            Line::from(vec![
                Span::styled("Requests: ", muted_style()),
                Span::styled(summary.total_requests.to_string(), text_style()),
            ]),
        ],
        _ => vec![
            Line::from(vec![Span::styled("No performance data yet", muted_style())]),
            Line::from(vec![Span::styled("Start chatting to collect metrics", muted_style())]),
        ],
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
            "user" => (accent_style(), text_style(), "❯ ", user_name.to_string()),
            "assistant" => (shark_style(), text_style(), "🦈 ", agent_name.to_string()),
            "system" => {
                // Use error styling for error messages
                let is_error = msg.content.contains("Error:")
                    || msg.content.contains("error:")
                    || msg.content.contains("Failed")
                    || msg.content.contains("failed");
                if is_error {
                    (error_style(), error_style(), "⚠ ", "system".to_string())
                } else {
                    (muted_style(), muted_style(), "ℹ ", "system".to_string())
                }
            }
            _ => (text_style(), text_style(), "  ", msg.role.clone()),
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, role_style),
            Span::styled(display_role, role_style.add_modifier(Modifier::BOLD)),
        ]));

        for content_line in msg.content.lines() {
            // Welcome logo lines use purple for visibility against dark bg
            let line_style = if msg.role == "system" && content_line.contains('█') {
                Style::default().fg(current_theme().accent).add_modifier(Modifier::BOLD)
            } else {
                content_style
            };
            lines.push(Line::from(vec![Span::styled(
                content_line,
                line_style,
            )]));
        }

        lines.push(Line::from(""));
    }

    if app.is_streaming {
        let agent_name = &app.config.agent.display_name;
        lines.push(Line::from(vec![
            Span::styled("🦈 ", shark_style()),
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
        let cursor_x = inner.x + app.cursor_position as u16;
        let cursor_y = inner.y;
        f.set_cursor_position((cursor_x, cursor_y));
    }
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

#[allow(dead_code)]
#[allow(dead_code)]
#[allow(dead_code)]
fn draw_chat_header(_f: &mut Frame, _area: Rect) {
    // Removed — welcome message is now in chat history
}
