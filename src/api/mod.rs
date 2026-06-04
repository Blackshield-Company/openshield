//! HTTP + WebSocket API for external tool integration.
//!
//! Exposes OpenShark's agent, tools, and diagnostics over a REST + WS API
//! behind the `web-api` feature flag.
//!
//! ## HTTP Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET    | /api/v1/health | Health check |
//! | GET    | /api/v1/tools | List available tools |
//! | POST   | /api/v1/tools/:name | Execute a tool |
//! | GET    | /api/v1/diagnostics | All LSP diagnostics |
//! | GET    | /api/v1/diagnostics/:file | Diagnostics for a file |
//! | GET    | /api/v1/sessions | List sessions |
//! | GET    | /api/v1/sessions/:id | Get session details |
//! | POST   | /api/v1/chat | Send a chat message (non-streaming) |
//! | POST   | /api/v1/agent | Run agent task (returns task ID) |
//! | GET    | /api/v1/agent/:id | Get agent task status |
//!
//! ## WebSocket
//!
//! | Path | Description |
//! |------|-------------|
//! | /ws/v1/chat | Streaming chat with real-time events |
//! | /ws/v1/agent | Streaming agent execution with tool/progress events |

pub mod handlers;
pub mod ws;

use axum::Router;
use axum::routing::{get, post};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<crate::config::Config>,
    pub running_tasks: Arc<RwLock<Vec<AgentTask>>>,
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

fn default_timeout() -> u64 { 300 }
fn default_max_turns() -> usize { 50 }

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
    Router::new()
        // Health
        .route("/api/v1/health", get(handlers::health))
        // Tools
        .route("/api/v1/tools", get(handlers::list_tools))
        .route("/api/v1/tools/{name}", post(handlers::execute_tool))
        // Diagnostics
        .route("/api/v1/diagnostics", get(handlers::all_diagnostics))
        .route("/api/v1/diagnostics/{*file}", get(handlers::file_diagnostics))
        // Chat
        .route("/api/v1/chat", post(handlers::chat))
        // Agent
        .route("/api/v1/agent", post(handlers::start_agent_task))
        .route("/api/v1/agent/{id}", get(handlers::get_agent_task))
        // WebSocket
        .route("/ws/v1/chat", get(ws::ws_chat))
        .route("/ws/v1/agent", get(ws::ws_agent))
        .with_state(state)
}

/// Start the API server on the given address.
pub async fn serve(state: AppState, addr: &str) -> anyhow::Result<()> {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("OpenShark API server listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
