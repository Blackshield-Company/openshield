//! HTTP + WebSocket API for external tool integration.
//!
//! Exposes OpenShield's agent, tools, and diagnostics over a REST + WS API
//! behind the `web-api` feature flag.
//!
//! ## HTTP Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET    | /api/v1/health | Health check (+ version, active model, theme) |
//! | GET    | /api/v1/models | Providers + models from config.toml (router slots) |
//! | GET    | /api/v1/doctor | Doctor report (reuses `openshield doctor` checks) |
//! | POST   | /api/v1/pair | Short-lived pairing payload {host, port, token, expires} |
//! | GET    | /api/v1/tools | List available tools |
//! | POST   | /api/v1/tools/:name | Execute a tool |
//! | GET    | /api/v1/diagnostics | All LSP diagnostics |
//! | GET    | /api/v1/diagnostics/:file | Diagnostics for a file |
//! | GET    | /api/v1/sessions | List sessions |
//! | GET    | /api/v1/sessions/:id | Session details + full history (messages, tool calls) |
//! | GET    | /api/v1/memory | Search persistent memory |
//! | GET    | /api/v1/memory/search | Alias of `/api/v1/memory` (design-doc URL, §3.1) |
//! | GET    | /api/v1/approvals | Pending security-gate approvals (M3, High/Critical risk) |
//! | POST   | /api/v1/approvals/:id | approve / reject / approve-always a pending approval |
//! | GET    | /api/v1/swarm | Swarm state (engine, agents, consensus, last event; offline state when idle) |
//! | POST   | /api/v1/swarm/start | Idempotent swarm start (`{seed, roles, max_agents}` optional) |
//! | POST   | /api/v1/swarm/stop | Idempotent swarm stop |
//! | POST   | /api/v1/chat | Send a chat message (non-streaming) |
//! | POST   | /api/v1/agent | Run agent task (returns task ID) |
//! | GET    | /api/v1/agent/:id | Get agent task status |
//!
//! ## WebSocket
//!
//! | Path | Description |
//! |------|-------------|
//! | /ws/v1/chat | Streaming chat with real-time events (typed envelope, §4 of the desktop design doc) |
//! | /ws/v1/agent | Streaming agent execution with tool/progress events |
//!
//! ## Auth
//!
//! `openshield serve` and [`crate::serve_in_process`] attach bearer-token auth
//! (see `auth`) and enforce it on every request, including loopback. Embedded
//! hosts obtain the token via [`crate::embedded_token`] and pass it to the
//! WebView (`Authorization: Bearer`, or `?token=` on WebSocket upgrades).
//! CORS is an origin allowlist (including `http://tauri.localhost` and
//! `https://tauri.localhost`); `Host` must be loopback or the bind address.

pub mod approvals;
pub mod auth;
pub mod handlers;
pub mod swarm;
pub mod ws;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub config: Arc<crate::config::Config>,
    pub running_tasks: Arc<RwLock<Vec<AgentTask>>>,
    /// Explicit config directory for embedded hosts (Android), where
    /// `dirs::config_dir()` is None. When set, per-request config reloads
    /// use `Config::load_from_dir` instead of `load_or_default`.
    pub config_dir: Option<std::path::PathBuf>,
    /// Bearer-token auth state. `None` disables enforcement.
    /// `openshield serve` and the embedded Pocket host always set this and
    /// require the token on every request, including loopback. WebSocket
    /// upgrades may pass `?token=`.
    pub auth: Option<Arc<auth::AuthState>>,
    /// Human-in-the-loop approval bridge (M3): pending High/Critical tool
    /// calls, the `/ws/v1/chat` approval event broadcast, and the WS client
    /// counter that decides whether the human gate engages.
    pub approvals: Arc<approvals::ApprovalsState>,
    /// Swarm engine handle (M5): the engine owned by this API server
    /// process, created on first `POST /api/v1/swarm/start` or attached by
    /// an embedded host. `GET /api/v1/swarm` reports a valid offline state
    /// while no engine exists.
    pub swarm: Arc<swarm::SwarmState>,
    /// Server start time, for real uptime reporting on /api/v1/health.
    pub started: std::time::Instant,
}

impl AppState {
    /// Reload config fresh from disk so edits (e.g. provider keys saved
    /// from a host app's config view) apply without a server restart.
    pub fn reload_config(&self) -> anyhow::Result<crate::config::Config> {
        match &self.config_dir {
            Some(dir) => crate::config::Config::load_from_dir(dir),
            None => crate::config::Config::load_or_default(),
        }
    }
}

/// A tracked agent task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: String,
    pub task: String,
    pub status: AgentTaskStatus,
    pub result: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskStatus {
    Running,
    Completed,
    Failed,
}

/// Chat request body.
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Chat response body.
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub message: String,
    pub model: String,
    pub session_id: String,
}

/// Agent task request body.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AgentTaskRequest {
    pub task: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default)]
    pub yolo: bool,
}

fn default_timeout() -> u64 {
    300
}
fn default_max_turns() -> usize {
    50
}

/// Tool execution request.
#[derive(Debug, Deserialize)]
pub struct ToolRequest {
    pub args: String,
}

/// Tool execution response.
#[derive(Debug, Serialize)]
pub struct ToolResponse {
    pub tool: String,
    pub output: String,
    pub success: bool,
}

/// Generic API error.
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

/// Build the complete API router.
pub fn build_router(state: AppState) -> Router {
    let origins = state.config.desktop.allowed_origins.clone();
    let auth_state = state.clone();
    let host_state = state.clone();
    Router::new()
        // Health
        .route("/api/v1/health", get(handlers::health))
        // Tools
        .route("/api/v1/tools", get(handlers::list_tools))
        .route("/api/v1/tools/{name}", post(handlers::execute_tool))
        // Diagnostics
        .route("/api/v1/diagnostics", get(handlers::all_diagnostics))
        .route(
            "/api/v1/diagnostics/{*file}",
            get(handlers::file_diagnostics),
        )
        // Stats / Models / Memory
        .route("/api/v1/stats", get(handlers::stats))
        .route("/api/v1/models", get(handlers::list_models))
        .route("/api/v1/memory", get(handlers::memory_search))
        // Alias so the design-doc URL works (§3.1 memory browser)
        .route("/api/v1/memory/search", get(handlers::memory_search))
        // Doctor + pairing (desktop/pocket companions, design doc §3.1)
        .route("/api/v1/doctor", get(handlers::doctor))
        .route("/api/v1/pair", post(handlers::pair))
        // Chat
        .route("/api/v1/chat", post(handlers::chat))
        // Chat sessions
        .route(
            "/api/v1/sessions",
            get(handlers::list_sessions).post(handlers::create_session),
        )
        .route(
            "/api/v1/sessions/{id}",
            get(handlers::get_session_detail).delete(handlers::close_session),
        )
        .route(
            "/api/v1/sessions/{id}/messages",
            get(handlers::session_messages),
        )
        // Approvals (M3, design doc §3.1/§4): the human gate over WS-held
        // High/Critical tool calls.
        .route("/api/v1/approvals", get(approvals::list_approvals))
        .route("/api/v1/approvals/{id}", post(approvals::resolve_approval))
        // Swarm (M5, design doc §3.1): monitor + start/stop from the GUI
        .route("/api/v1/swarm", get(swarm::get_swarm))
        .route("/api/v1/swarm/start", post(swarm::start_swarm))
        .route("/api/v1/swarm/stop", post(swarm::stop_swarm))
        // Agent
        .route("/api/v1/agent", post(handlers::start_agent_task))
        .route("/api/v1/agent/{id}", get(handlers::get_agent_task))
        // WebSocket
        .route("/ws/v1/chat", get(ws::ws_chat))
        .route("/ws/v1/agent", get(ws::ws_agent))
        .with_state(state)
        // Bearer token whenever `auth` is Some, including loopback.
        .layer(middleware::from_fn_with_state(
            auth_state,
            auth::require_bearer,
        ))
        // DNS-rebinding: Host must be loopback, the bind address, or an
        // explicit allowlist entry. Inside CORS so preflight is answered first.
        .layer(middleware::from_fn_with_state(
            host_state,
            auth::require_allowed_host,
        ))
        // Outermost: answers preflight before auth or Host checks.
        .layer(auth::cors_layer(origins))
}

/// Start the API server on the given address.
pub async fn serve(state: AppState, addr: &str) -> anyhow::Result<()> {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("OpenShield API server listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
