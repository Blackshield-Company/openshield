//! Legacy HTTP gateway (default 127.0.0.1:9876).
//!
//! Started only by `openshield server` or by an explicit TUI opt-in
//! (`--tui-http` or `[desktop] tui_http = true`). Pocket and Desktop should
//! use `openshield serve` (port 9310) instead.
//!
//! Every request requires the same bearer token as `openshield serve`
//! (`Authorization: Bearer`, token file `[desktop] token_path`). Non-loopback
//! binds are refused unless `[desktop] lan_mode = true`. Host and CORS
//! follow the serve allowlists.

use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::api::auth::AuthState;
use crate::config::Config;
use crate::memory::{MemoryStore, Message as MemoryMessage};
use crate::providers::{ChatRequest, Message, Provider};

/// Shared application state.
pub struct AppState {
    pub config: Config,
    pub memory: Mutex<MemoryStore>,
    pub auth: Arc<AuthState>,
}

impl AppState {
    pub fn new(config: Config, auth: Arc<AuthState>) -> anyhow::Result<Arc<Self>> {
        let memory = MemoryStore::new(&config.memory_db_path)?;

        Ok(Arc::new(Self {
            config,
            memory: Mutex::new(memory),
            auth,
        }))
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized: Bearer token required" })),
    )
        .into_response()
}

/// Same rule as `openshield serve`: Bearer token on every request,
/// including loopback.
async fn require_bearer(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    match crate::api::auth::bearer_from_headers(req.headers()) {
        Some(token) if state.auth.accepts(token) => next.run(req).await,
        _ => unauthorized(),
    }
}

async fn require_allowed_host(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(raw) = req
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        && !crate::api::auth::host_allowed(raw, &state.config.desktop)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid host" })),
        )
            .into_response();
    }
    next.run(req).await
}

fn bind_is_loopback(bind: &str) -> bool {
    if let Ok(ip) = bind.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    matches!(
        bind.to_ascii_lowercase().as_str(),
        "localhost" | "ip6-localhost"
    )
}

/// Chat request body.
#[derive(Debug, Deserialize)]
pub struct ChatRequestBody {
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

/// Chat response chunk (SSE).
#[derive(Debug, Serialize)]
pub struct ChatResponseChunk {
    pub chunk: String,
    #[serde(default)]
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub args: String,
    pub result: Option<String>,
}

/// Memory search request.
#[derive(Debug, Deserialize)]
pub struct MemorySearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub semantic: bool,
}

fn default_limit() -> usize {
    10
}

/// Memory save request.
#[derive(Debug, Deserialize)]
pub struct MemorySaveRequest {
    pub content: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Model info response.
#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub name: String,
    pub provider: String,
    pub context_length: usize,
    pub cost_per_1k_input: f64,
    pub cost_per_1k_output: f64,
    pub capabilities: Vec<String>,
}

/// Status response.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub version: String,
    pub default_model: String,
    pub models_available: usize,
    pub memory_enabled: bool,
    pub uptime_seconds: u64,
}

/// Tool execution request.
#[derive(Debug, Deserialize)]
pub struct ToolExecuteRequest {
    pub name: String,
    pub args: String,
}

/// Tool execution response.
#[derive(Debug, Serialize)]
pub struct ToolExecuteResponse {
    pub success: bool,
    pub result: String,
    pub error: Option<String>,
}

/// Build the HTTP router.
pub fn build_router(state: Arc<AppState>) -> Router {
    let origins = state.config.desktop.allowed_origins.clone();
    let auth_state = state.clone();
    let host_state = state.clone();
    Router::new()
        .route("/v1/chat", post(handle_chat))
        .route("/v1/models", get(handle_models))
        .route(
            "/v1/memory",
            get(handle_memory_search).post(handle_memory_save),
        )
        .route("/v1/status", get(handle_status))
        .route("/v1/tools/execute", post(handle_tool_execute))
        .route("/v1/health", get(handle_health))
        .with_state(state)
        .layer(middleware::from_fn_with_state(auth_state, require_bearer))
        .layer(middleware::from_fn_with_state(
            host_state,
            require_allowed_host,
        ))
        .layer(crate::api::auth::cors_layer(origins))
}

/// POST /v1/chat — Main chat endpoint with SSE streaming.
async fn handle_chat(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatRequestBody>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = body
        .session_id
        .unwrap_or_else(|| "mobile-default".to_string());
    let model = body
        .model
        .unwrap_or_else(|| state.config.default_model.clone());
    let system_prompt = body.system_prompt;

    let (tx, mut rx) = mpsc::unbounded_channel::<ChatResponseChunk>();

    // Spawn the chat processing in a background task.
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        if let Err(e) = process_chat(
            state_clone,
            body.message,
            model,
            session_id,
            system_prompt,
            tx,
        )
        .await
        {
            error!("Chat processing error: {}", e);
        }
    });

    // Stream chunks back as SSE events.
    let stream = async_stream::stream! {
        while let Some(chunk) = rx.recv().await {
            let event = Event::default()
                .event("message")
                .data(serde_json::to_string(&chunk).unwrap_or_default());
            yield Ok(event);
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

/// Process a chat message and stream chunks back.
async fn process_chat(
    state: Arc<AppState>,
    content: String,
    model: String,
    session_id: String,
    _system_prompt: Option<String>,
    tx: mpsc::UnboundedSender<ChatResponseChunk>,
) -> anyhow::Result<()> {
    // Find provider for model.
    let (provider_name, provider_cfg) = state
        .config
        .find_provider_for_model(&model)
        .ok_or_else(|| anyhow::anyhow!("Model '{}' not found in config", model))?;

    let provider = Provider::new(
        provider_name.clone(),
        provider_cfg.base_url.clone(),
        provider_cfg.api_key.clone(),
        provider_cfg.kind.clone(),
        provider_cfg.headers.clone(),
    );

    // Build message history from memory.
    let mut messages = vec![];

    // Add system prompt if configured.
    let agent = &state.config.agent;
    messages.push(Message {
        role: "system".to_string(),
        content: format!(
            "You are {}. {} {}",
            agent.display_name, agent.purpose, agent.tagline
        ),
        images: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    });

    // Fetch recent memory context.
    match state.memory.lock().unwrap().search_messages(&content, 5) {
        Ok(memories) if !memories.is_empty() => {
            let context = memories
                .iter()
                .map(|m| {
                    format!(
                        "[{}] {}: {}",
                        m.created_at.format("%Y-%m-%d"),
                        m.role,
                        m.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(Message {
                role: "system".to_string(),
                content: format!("[RELEVANT MEMORY]\n{}\n[END MEMORY]", context),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
        }
        _ => {}
    }

    // Add user message.
    messages.push(Message {
        role: "user".to_string(),
        content: content.clone(),
        images: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    });

    // Save user message to memory.
    let _ = state.memory.lock().unwrap().save_message(&MemoryMessage {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        role: "user".to_string(),
        content: content.clone(),
        created_at: chrono::Utc::now(),
        tokens_used: None,
    });

    // Stream response from provider.
    let req = ChatRequest::new(model, messages, true);

    match provider.chat_stream(req).await {
        Ok((chunks, _metrics)) => {
            let full_response = chunks.join("");

            // Stream each chunk.
            let chunk_count = chunks.len();
            for (i, chunk) in chunks.into_iter().enumerate() {
                let is_done = i + 1 == chunk_count;
                let _ = tx.send(ChatResponseChunk {
                    chunk,
                    done: is_done,
                    tool_calls: None,
                    error: None,
                });
            }

            // Save assistant response to memory.
            let _ = state.memory.lock().unwrap().save_message(&MemoryMessage {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.clone(),
                role: "assistant".to_string(),
                content: full_response.clone(),
                created_at: chrono::Utc::now(),
                tokens_used: None,
            });

            // Check for tool calls in response.
            // (Simplified — full implementation would parse TOOL: invocations)
        }
        Err(e) => {
            let _ = tx.send(ChatResponseChunk {
                chunk: String::new(),
                done: true,
                tool_calls: None,
                error: Some(format!("Provider error: {}", e)),
            });
        }
    }

    Ok(())
}

/// GET /v1/models — List available models.
async fn handle_models(State(state): State<Arc<AppState>>) -> Json<Vec<ModelInfo>> {
    let mut models = Vec::new();

    for (provider_name, provider) in &state.config.providers {
        for model in &provider.models {
            models.push(ModelInfo {
                name: model.name.clone(),
                provider: provider_name.clone(),
                context_length: model.context_length,
                cost_per_1k_input: model.cost_per_1k_input,
                cost_per_1k_output: model.cost_per_1k_output,
                capabilities: model.capabilities.clone(),
            });
        }
    }

    Json(models)
}

/// GET /v1/memory — Search memory.
async fn handle_memory_search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<MemorySearchRequest>,
) -> Json<Vec<MemoryMessage>> {
    let results = if params.semantic {
        state
            .memory
            .lock()
            .unwrap()
            .semantic_search(&params.query, params.limit)
            .unwrap_or_default()
            .into_iter()
            .map(|(msg, _)| msg)
            .collect()
    } else {
        state
            .memory
            .lock()
            .unwrap()
            .search_messages(&params.query, params.limit)
            .unwrap_or_default()
    };

    Json(results)
}

/// POST /v1/memory — Save to memory.
async fn handle_memory_save(
    State(state): State<Arc<AppState>>,
    Json(body): Json<MemorySaveRequest>,
) -> Json<serde_json::Value> {
    let session_id = body
        .session_id
        .unwrap_or_else(|| "mobile-default".to_string());

    match state.memory.lock().unwrap().save_message(&MemoryMessage {
        id: uuid::Uuid::new_v4().to_string(),
        session_id,
        role: "user".to_string(),
        content: body.content,
        created_at: chrono::Utc::now(),
        tokens_used: None,
    }) {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

/// GET /v1/status — System status.
async fn handle_status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let models_count: usize = state
        .config
        .providers
        .values()
        .map(|p| p.models.len())
        .sum();

    Json(StatusResponse {
        version: state.config.version.clone(),
        default_model: state.config.default_model.clone(),
        models_available: models_count,
        memory_enabled: true,
        uptime_seconds: 0, // TODO: Track actual uptime
    })
}

/// POST /v1/tools/execute — Execute a tool directly.
async fn handle_tool_execute(Json(body): Json<ToolExecuteRequest>) -> Json<ToolExecuteResponse> {
    use crate::tools::find_tool;

    match find_tool(&body.name) {
        Some(tool) => match tool.execute(&body.args) {
            Ok(result) => Json(ToolExecuteResponse {
                success: true,
                result,
                error: None,
            }),
            Err(e) => Json(ToolExecuteResponse {
                success: false,
                result: String::new(),
                error: Some(e.to_string()),
            }),
        },
        None => Json(ToolExecuteResponse {
            success: false,
            result: String::new(),
            error: Some(format!("Tool '{}' not found", body.name)),
        }),
    }
}

/// GET /v1/health — Health check.
async fn handle_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "openshield" }))
}

/// Start the HTTP server.
pub async fn start_server(config: Config, bind: String, port: u16) -> anyhow::Result<()> {
    if !bind_is_loopback(&bind) && !config.desktop.lan_mode {
        anyhow::bail!(
            "Refusing to bind {bind}:{port}: non-loopback addresses require lan_mode = true in [desktop]"
        );
    }

    let token = crate::api::auth::ensure_token_file(&config.desktop.token_path)?;
    let auth = AuthState::new(token, true);

    let mut config = config;
    config.desktop.bind = bind.clone();
    config.desktop.port = port;

    let addr = format!("{}:{}", bind, port);
    info!(
        "OpenShield HTTP server starting on http://{} (Bearer token required, token file {})",
        addr, config.desktop.token_path
    );

    let state = AppState::new(config, auth)?;
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_app(token: &str) -> Router {
        let mut config = Config::default();
        let dir = std::env::temp_dir().join(format!(
            "openshield-gw-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        config.memory_db_path = dir.join("memory.db");
        let auth = AuthState::new(token.to_string(), true);
        let state = AppState::new(config, auth).expect("gateway state");
        build_router(state)
    }

    async fn status(app: &Router, method: &str, uri: &str, token: Option<&str>) -> StatusCode {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let req = builder.body(Body::empty()).unwrap();
        app.clone().oneshot(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn health_requires_bearer_token() {
        let app = test_app("master-token");
        assert_eq!(
            status(&app, "GET", "/v1/health", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "GET", "/v1/health", Some("wrong")).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "GET", "/v1/health", Some("master-token")).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn tool_execute_requires_bearer_token() {
        let app = test_app("master-token");
        assert_eq!(
            status(&app, "POST", "/v1/tools/execute", None).await,
            StatusCode::UNAUTHORIZED
        );
        // A request with the token is authenticated; a missing body may still
        // be rejected as a bad request, but it must not be 401.
        let status = status(&app, "POST", "/v1/tools/execute", Some("master-token")).await;
        assert_ne!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn loopback_detection() {
        assert!(bind_is_loopback("127.0.0.1"));
        assert!(bind_is_loopback("::1"));
        assert!(bind_is_loopback("localhost"));
        assert!(bind_is_loopback("127.0.0.1"));
        assert!(!bind_is_loopback("0.0.0.0"));
        assert!(!bind_is_loopback("192.168.1.10"));
    }
}
