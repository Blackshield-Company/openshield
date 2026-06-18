//! HTTP request handlers for the OpenShark API.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;

use super::{
    AgentTask, AgentTaskStatus, ApiError, AppState, ChatRequest as ApiChatRequest, ChatResponse,
    ToolRequest, ToolResponse,
};

/// GET /api/v1/health
pub async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": chrono::Utc::now().timestamp(),
    }))
}

/// GET /api/v1/tools
pub async fn list_tools() -> Json<serde_json::Value> {
    let tools = crate::tools::get_openai_tool_definitions();
    Json(json!({
        "tools": tools,
        "count": tools.len(),
    }))
}

/// POST /api/v1/tools/:name
pub async fn execute_tool(Path(name): Path<String>, body: Json<ToolRequest>) -> impl IntoResponse {
    let security = match crate::security::SecurityEngine::new(
        crate::security::SecurityConfig::load().unwrap_or_default()
    ) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("Security engine init failed: {}", e),
                }),
            )
                .into_response();
        }
    };

    match security.check_tool_call(&name, &body.args) {
        crate::security::SecurityDecision::Allow => {}
        crate::security::SecurityDecision::RequireApproval { reason, .. } => {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiError {
                    error: format!("Tool '{}' requires approval: {}", name, reason),
                }),
            )
                .into_response();
        }
        crate::security::SecurityDecision::Deny { reason } => {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiError {
                    error: format!("Tool '{}' denied: {}", name, reason),
                }),
            )
                .into_response();
        }
    }

    let result = if let Some(async_tool) = crate::tools::find_async_tool(&name) {
        async_tool.execute_async(&body.args).await
    } else if let Some(tool) = crate::tools::find_tool(&name) {
        tool.execute(&body.args)
    } else {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Unknown tool: {}", name),
            }),
        )
            .into_response();
    };

    match result {
        Ok(output) => Json(ToolResponse {
            tool: name.clone(),
            output: security.sanitize_output(&name, &output),
            success: true,
        })
        .into_response(),
        Err(e) => Json(ToolResponse {
            tool: name,
            output: e.to_string(),
            success: false,
        })
        .into_response(),
    }
}

/// GET /api/v1/diagnostics
pub async fn all_diagnostics() -> Json<serde_json::Value> {
    let manager = crate::lsp::global_lsp_manager();
    let store = manager.diagnostics_store();
    let all = store.get_all().await;

    let total: usize = all.values().map(|v| v.len()).sum();
    let files: Vec<serde_json::Value> = all
        .iter()
        .map(|(uri, diags)| {
            let issues: Vec<serde_json::Value> = diags
                .iter()
                .map(|d| {
                    json!({
                        "severity": d.severity,
                        "message": d.message,
                        "line": d.line,
                        "character": d.character,
                    })
                })
                .collect();
            json!({
                "uri": uri,
                "count": diags.len(),
                "diagnostics": issues,
            })
        })
        .collect();

    Json(json!({
        "total": total,
        "files": files.len(),
        "diagnostics": files,
    }))
}

/// GET /api/v1/diagnostics/*file
pub async fn file_diagnostics(Path(file): Path<String>) -> Json<serde_json::Value> {
    let manager = crate::lsp::global_lsp_manager();
    let store = manager.diagnostics_store();

    let uri = if file.starts_with("file://") {
        file.clone()
    } else {
        format!("file://{}", file)
    };

    let diags = store.get(&uri).await;
    let issues: Vec<serde_json::Value> = diags
        .iter()
        .map(|d| {
            json!({
                "severity": d.severity,
                "message": d.message,
                "line": d.line,
                "character": d.character,
            })
        })
        .collect();

    Json(json!({
        "uri": uri,
        "count": diags.len(),
        "diagnostics": issues,
    }))
}

/// POST /api/v1/chat
pub async fn chat(body: Json<ApiChatRequest>) -> impl IntoResponse {
    let config = match crate::config::Config::load_or_default() {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "Failed to load config".to_string(),
                }),
            )
                .into_response();
        }
    };
    let model = body
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());
    let session_id = body
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Get first configured provider
    let (provider_name, provider_config) = match config.providers.iter().next() {
        Some((n, p)) => (n.clone(), p.clone()),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "No providers configured".to_string(),
                }),
            )
                .into_response();
        }
    };

    let provider = crate::providers::Provider::new(
        provider_name,
        provider_config.base_url,
        provider_config.api_key,
        provider_config.kind,
        provider_config.headers,
    );

    let request = crate::providers::ChatRequest::new(
        model.clone(),
        vec![crate::providers::Message {
            role: "user".to_string(),
            content: body.message.clone(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }],
        false,
    );

    match provider.chat(request).await {
        Ok(response) => {
            let content = response
                .choices
                .first()
                .map(|c| c.message.content.clone())
                .unwrap_or_default();
            Json(ChatResponse {
                message: content,
                model,
                session_id,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("Chat failed: {}", e),
            }),
        )
            .into_response(),
    }
}

/// POST /api/v1/agent
pub async fn start_agent_task(
    State(state): State<AppState>,
    body: Json<super::AgentTaskRequest>,
) -> impl IntoResponse {
    let task_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let task = AgentTask {
        id: task_id.clone(),
        task: body.task.clone(),
        status: AgentTaskStatus::Running,
        result: None,
        created_at: now.clone(),
        updated_at: now,
    };

    {
        let mut tasks = state.running_tasks.write().await;
        tasks.push(task);
    }

    // Spawn the headless agent in the background and update the task when done
    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    let task_str = body.task.clone();
    let model_override = body.model.clone();
    let timeout_secs = body.timeout_secs;
    let max_turns = body.max_turns;
    let yolo = body.yolo;
    tokio::spawn(async move {
        let config = match crate::config::Config::load_or_default() {
            Ok(c) => c,
            Err(e) => {
                let mut tasks = state_clone.running_tasks.write().await;
                if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id_clone) {
                    t.status = AgentTaskStatus::Failed;
                    t.result = Some(format!("Config load failed: {}", e));
                    t.updated_at = chrono::Utc::now().to_rfc3339();
                }
                return;
            }
        };

        let (provider_name, provider_config) = match config.providers.iter().next() {
            Some((n, p)) => (n.clone(), p.clone()),
            None => {
                let mut tasks = state_clone.running_tasks.write().await;
                if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id_clone) {
                    t.status = AgentTaskStatus::Failed;
                    t.result = Some("No providers configured".to_string());
                    t.updated_at = chrono::Utc::now().to_rfc3339();
                }
                return;
            }
        };

        let provider = crate::providers::Provider::new(
            provider_name,
            provider_config.base_url,
            provider_config.api_key,
            provider_config.kind,
            provider_config.headers,
        );

        let model = model_override.unwrap_or_else(|| config.default_model.clone());
        let headless_config = crate::headless::HeadlessConfig {
            task: task_str,
            yolo,
            autonomous: false,
            json: false,
            timeout_secs,
            max_turns,
            model: Some(model.clone()),
            output_file: None,
        };

        let security = match crate::security::SecurityEngine::new(
            crate::security::SecurityConfig::load().unwrap_or_default()
        ) {
            Ok(s) => s,
            Err(e) => {
                let mut tasks = state_clone.running_tasks.write().await;
                if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id_clone) {
                    t.status = AgentTaskStatus::Failed;
                    t.result = Some(format!("Security engine failed: {}", e));
                    t.updated_at = chrono::Utc::now().to_rfc3339();
                }
                return;
            }
        };

        let result = crate::headless::run_headless(
            headless_config,
            provider,
            model,
            security,
            None,
        ).await;

        let mut tasks = state_clone.running_tasks.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id_clone) {
            match result {
                Ok(summary) => {
                    t.status = AgentTaskStatus::Completed;
                    t.result = Some(summary);
                }
                Err(e) => {
                    t.status = AgentTaskStatus::Failed;
                    t.result = Some(format!("Error: {}", e));
                }
            }
            t.updated_at = chrono::Utc::now().to_rfc3339();
        }
    });

    (StatusCode::ACCEPTED, Json(json!({ "task_id": task_id })))
}

/// GET /api/v1/agent/:id
pub async fn get_agent_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tasks = state.running_tasks.read().await;
    match tasks.iter().find(|t| t.id == id) {
        Some(t) => Json(t.clone()).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Task not found: {}", id),
            }),
        )
            .into_response(),
    }
}
