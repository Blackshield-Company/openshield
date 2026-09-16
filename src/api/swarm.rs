//! Swarm monitor + control endpoints (M5, design doc §3.1).
//!
//! Gives the desktop/pocket GUI a read view of the swarm engine plus
//! start/stop control, without touching the TUI or headless paths:
//!
//! - `GET  /api/v1/swarm`        — engine state, agents, consensus doc,
//!                                 last event. Always 200: with no swarm
//!                                 active it returns a valid offline state
//!                                 (`running: false`, empty arrays) instead
//!                                 of 404/500, so the GUI can render a
//!                                 disabled panel without special-casing.
//! - `POST /api/v1/swarm/start`  — idempotent start; creates an engine from
//!                                 `[swarm]` config on first use (optional
//!                                 JSON body: `{seed, roles, max_agents}`).
//! - `POST /api/v1/swarm/stop`   — idempotent stop.
//!
//! Both POSTs return `{success, state, message}`; failures are structured
//! `ApiError` JSON. The engine itself is the existing `crate::swarm`
//! module — the same `SwarmEngine` the TUI drives via `/swarm` — so event
//! semantics, consensus memory, and the security posture are unchanged.
//!
//! Embedded hosts (Android) that already own a `SwarmEngine` can attach it
//! with [`SwarmState::attach`] so the API reports and controls theirs.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::RwLock as TokioRwLock;

use super::{ApiError, AppState};
use crate::swarm::{AgentStatus, SwarmConfig, SwarmEngine, SwarmEvent};

/// Default seed when POST /start is called without a body — the phone UI's
/// stop/start button doesn't type a prompt, and an idle-but-initialized
/// swarm still beats a 400.
const DEFAULT_SEED: &str = "Autonomous swarm session started from the OpenShield GUI";

/// Shared handle to the swarm engine living in the API server process.
/// `None` until a swarm is started via `POST /api/v1/swarm/start` (or an
/// embedded host attaches one with [`SwarmState::attach`]).
pub struct SwarmState {
    engine: TokioRwLock<Option<Arc<SwarmEngine>>>,
    /// Most recent event seen on the engine's broadcast stream, as a
    /// GUI-friendly `{type, agent, at}` object. `Arc` because the watcher
    /// task shares it (tokio's RwLock is not Clone).
    last_event: Arc<TokioRwLock<Option<Value>>>,
}

impl SwarmState {
    pub fn new() -> Self {
        Self {
            engine: TokioRwLock::new(None),
            last_event: Arc::new(TokioRwLock::new(None)),
        }
    }

    /// The attached engine, if any.
    pub async fn engine(&self) -> Option<Arc<SwarmEngine>> {
        self.engine.read().await.clone()
    }

    /// Attach an externally-owned engine (embedded hosts). Subscribes to its
    /// broadcast stream so `GET /api/v1/swarm` can report last activity.
    pub async fn attach(&self, engine: Arc<SwarmEngine>) {
        self.watch(engine.clone());
        *self.engine.write().await = Some(engine);
    }

    /// Spawn the last-event tracker for an engine.
    fn watch(&self, engine: Arc<SwarmEngine>) {
        let last_event = self.last_event.clone();
        let mut rx = engine.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                *last_event.write().await = Some(json!({
                    "type": event_label(&event),
                    "agent": event_agent(&event),
                    "at": chrono::Utc::now().to_rfc3339(),
                }));
            }
        });
    }
}

impl Default for SwarmState {
    fn default() -> Self {
        Self::new()
    }
}

/// Short snake_case label for a swarm event (GUI event feed).
fn event_label(event: &SwarmEvent) -> &'static str {
    match event {
        SwarmEvent::WorkCompleted { .. } => "work_completed",
        SwarmEvent::ReviewRequested { .. } => "review_requested",
        SwarmEvent::ReviewCompleted { .. } => "review_completed",
        SwarmEvent::ConsensusReached { .. } => "consensus_reached",
        SwarmEvent::AgentError { .. } => "agent_error",
        SwarmEvent::Heartbeat { .. } => "heartbeat",
        SwarmEvent::AgentActivity { .. } => "activity",
        SwarmEvent::AgentToolCall { .. } => "tool_call",
        SwarmEvent::AgentToolResult { .. } => "tool_result",
        SwarmEvent::AgentThinking { .. } => "thinking",
        SwarmEvent::AgentChunk { .. } => "chunk",
        SwarmEvent::Shutdown => "shutdown",
    }
}

/// Agent id associated with an event, when there is one.
fn event_agent(event: &SwarmEvent) -> Option<String> {
    match event {
        SwarmEvent::WorkCompleted { agent_id, .. }
        | SwarmEvent::AgentError { agent_id, .. }
        | SwarmEvent::Heartbeat { agent_id, .. }
        | SwarmEvent::AgentActivity { agent_id, .. }
        | SwarmEvent::AgentToolCall { agent_id, .. }
        | SwarmEvent::AgentToolResult { agent_id, .. }
        | SwarmEvent::AgentThinking { agent_id, .. }
        | SwarmEvent::AgentChunk { agent_id, .. } => Some(agent_id.clone()),
        SwarmEvent::ReviewRequested { from_agent, .. } => Some(from_agent.clone()),
        SwarmEvent::ReviewCompleted { reviewer, .. } => Some(reviewer.clone()),
        SwarmEvent::ConsensusReached { .. } | SwarmEvent::Shutdown => None,
    }
}

/// GUI-friendly agent view: role, lowercase status, model, and current task
/// when the agent is working.
fn agent_view(agent: &crate::swarm::SwarmAgent) -> Value {
    let (status, task) = match &agent.status {
        AgentStatus::Idle => ("idle", None),
        AgentStatus::Working { task, .. } => ("working", Some(task.clone())),
        AgentStatus::Reviewing { target_agent, .. } => {
            ("reviewing", Some(format!("reviewing {}", target_agent)))
        }
        AgentStatus::WaitingForConsensus { .. } => ("waiting_for_consensus", None),
        AgentStatus::Error { message } => ("error", Some(message.clone())),
        AgentStatus::Completed { result } => ("completed", Some(result.clone())),
    };
    json!({
        "id": agent.id,
        "name": agent.name,
        "role": agent.role.name,
        "status": status,
        "model": agent.model,
        "task": task,
        "last_activity": agent.last_activity,
        "cycles_completed": agent.cycles_completed,
        "errors_count": agent.errors_count,
        "success_rate": agent.success_rate,
    })
}

/// Build the GUI swarm payload. Always a valid object: with no engine
/// attached it is the offline state, never an error.
pub(crate) async fn swarm_snapshot(swarm: &SwarmState, enabled: bool) -> Value {
    let Some(engine) = swarm.engine().await else {
        return json!({
            "running": false,
            "enabled": enabled,
            "agent_count": 0,
            "agents": [],
            "consensus": {
                "entry_count": 0,
                "pending": 0,
                "approved": 0,
                "rejected": 0,
                "entries": [],
            },
            "cycles": { "completed": 0, "limit": 0 },
            "last_event": Value::Null,
        });
    };

    let status = engine.status().await;
    let agents = engine.agent_snapshot().await;
    let consensus = engine.consensus_snapshot().await;

    let entries: Vec<Value> = consensus
        .iter()
        .map(|e| {
            let status = match e.status {
                crate::swarm::consensus::EntryStatus::Pending => "pending",
                crate::swarm::consensus::EntryStatus::Approved => "approved",
                crate::swarm::consensus::EntryStatus::Rejected => "rejected",
            };
            json!({
                "id": e.id,
                "author": e.author,
                "task": e.task,
                "status": status,
                "approvals": e.approvals.len(),
                "rejections": e.rejections.len(),
                "timestamp": e.timestamp,
            })
        })
        .collect();

    let pending = entries
        .iter()
        .filter(|e| e["status"] == "pending")
        .count();
    let approved = entries
        .iter()
        .filter(|e| e["status"] == "approved")
        .count();
    let rejected = entries
        .iter()
        .filter(|e| e["status"] == "rejected")
        .count();

    json!({
        "running": status.running,
        "enabled": enabled,
        "agent_count": agents.len(),
        "agents": agents.iter().map(agent_view).collect::<Vec<_>>(),
        "consensus": {
            "entry_count": consensus.len(),
            "pending": pending,
            "approved": approved,
            "rejected": rejected,
            "entries": entries,
        },
        "cycles": {
            "completed": status.cycles_completed,
            "limit": status.cycle_limit,
        },
        "last_event": swarm.last_event.read().await.clone().unwrap_or(Value::Null),
    })
}

/// GET /api/v1/swarm — swarm state for the GUI. Never 404: offline is a
/// valid state.
pub async fn get_swarm(State(state): State<AppState>) -> impl IntoResponse {
    let enabled = state.config.swarm.enabled;
    Json(swarm_snapshot(&state.swarm, enabled).await)
}

/// Optional body for POST /api/v1/swarm/start.
#[derive(Debug, Deserialize)]
pub struct SwarmStartRequest {
    /// Seed prompt for a newly-created swarm.
    #[serde(default)]
    pub seed: Option<String>,
    /// Role override (otherwise `[swarm].roles` from config.toml).
    #[serde(default)]
    pub roles: Option<Vec<String>>,
    /// Agent cap override (otherwise `[swarm].max_agents`).
    #[serde(default)]
    pub max_agents: Option<usize>,
}

/// POST /api/v1/swarm/start — idempotent. Starts the attached engine, or
/// creates one from `[swarm]` config (plus optional body overrides) on
/// first use.
pub async fn start_swarm(
    State(state): State<AppState>,
    body: Option<Json<SwarmStartRequest>>,
) -> impl IntoResponse {
    let config = state
        .reload_config()
        .unwrap_or_else(|_| state.config.as_ref().clone());
    let (seed, roles, max_agents) = body
        .as_ref()
        .map(|Json(b)| (b.seed.clone(), b.roles.clone(), b.max_agents))
        .unwrap_or((None, None, None));

    // Fresh engine when none exists yet.
    if state.swarm.engine().await.is_none() {
        let mut swarm_config: SwarmConfig = config.swarm.clone();
        if let Some(roles) = roles {
            if roles.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        error: "roles must not be empty".to_string(),
                    }),
                )
                    .into_response();
            }
            swarm_config.roles = roles;
        }
        if let Some(max) = max_agents {
            if max == 0 {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        error: "max_agents must be >= 1".to_string(),
                    }),
                )
                    .into_response();
            }
            swarm_config.max_agents = max;
        }

        let engine = Arc::new(SwarmEngine::new(swarm_config));
        let seed = seed.unwrap_or_else(|| DEFAULT_SEED.to_string());
        if let Err(e) = engine.init(&seed, &config).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("swarm init failed: {}", e),
                }),
            )
                .into_response();
        }
        state.swarm.attach(engine).await;
    }

    let engine = match state.swarm.engine().await {
        Some(e) => e,
        None => unreachable!("engine attached above"),
    };

    let message = if engine.is_running().await {
        "swarm already running".to_string()
    } else if let Err(e) = engine.start().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("swarm start failed: {}", e),
            }),
        )
            .into_response();
    } else {
        "swarm started".to_string()
    };

    let snapshot = swarm_snapshot(&state.swarm, config.swarm.enabled).await;
    Json(json!({
        "success": true,
        "state": snapshot,
        "message": message,
    }))
    .into_response()
}

/// POST /api/v1/swarm/stop — idempotent; stopping an inactive swarm is a
/// success with the offline state, not an error (the phone UI's stop
/// button must survive double-taps).
pub async fn stop_swarm(State(state): State<AppState>) -> impl IntoResponse {
    let enabled = state.config.swarm.enabled;

    let message = match state.swarm.engine().await {
        Some(engine) if engine.is_running().await => {
            if let Err(e) = engine.stop().await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError {
                        error: format!("swarm stop failed: {}", e),
                    }),
                )
                    .into_response();
            }
            "swarm stopped"
        }
        _ => "swarm is not running",
    };

    let snapshot = swarm_snapshot(&state.swarm, enabled).await;
    Json(json!({
        "success": true,
        "state": snapshot,
        "message": message,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        AppState {
            config: Arc::new(crate::config::Config::default()),
            running_tasks: Arc::new(TokioRwLock::new(Vec::new())),
            config_dir: None,
            auth: None,
            approvals: Arc::new(super::super::approvals::ApprovalsState::new()),
            swarm: Arc::new(SwarmState::new()),
            started: std::time::Instant::now(),
        }
    }

    #[tokio::test]
    async fn test_offline_snapshot_is_valid_empty_state() {
        let swarm = SwarmState::new();
        let snap = swarm_snapshot(&swarm, false).await;

        // GUI-friendly offline shape — never 404/500 material.
        assert_eq!(snap["running"], false);
        assert_eq!(snap["enabled"], false);
        assert_eq!(snap["agent_count"], 0);
        assert!(snap["agents"].is_array());
        assert_eq!(snap["agents"].as_array().unwrap().len(), 0);
        assert_eq!(snap["consensus"]["entry_count"], 0);
        assert_eq!(snap["cycles"]["completed"], 0);
        assert!(snap["last_event"].is_null());
    }

    #[tokio::test]
    async fn test_get_swarm_handler_returns_200_offline() {
        let state = test_state();
        let response = get_swarm(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_start_stop_cycle() {
        let state = test_state();

        // Start with no engine attached → creates one from default config.
        let response = start_swarm(State(state.clone()), None)
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        // Default config spawns the four default roles.
        let snap = swarm_snapshot(&state.swarm, false).await;
        assert_eq!(snap["running"], true);
        assert_eq!(snap["agent_count"], 4);
        let agents = snap["agents"].as_array().unwrap();
        assert!(agents.iter().all(|a| a["role"].is_string()));
        assert!(agents.iter().any(|a| a["role"] == "Architect"));

        // Stop → success with the (now idle) engine state still attached.
        let response = stop_swarm(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let snap = swarm_snapshot(&state.swarm, false).await;
        assert_eq!(snap["running"], false);
        assert_eq!(snap["agent_count"], 4);

        // Double-stop is idempotent, not an error.
        let response = stop_swarm(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_start_with_body_overrides() {
        let state = test_state();
        let body = Json(SwarmStartRequest {
            seed: Some("Build a REST API".to_string()),
            roles: Some(vec!["architect".to_string(), "tester".to_string()]),
            max_agents: Some(2),
        });

        start_swarm(State(state.clone()), Some(body))
            .await
            .into_response();
        let snap = swarm_snapshot(&state.swarm, false).await;
        assert_eq!(snap["agent_count"], 2);
        let mut roles: Vec<&str> = snap["agents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["role"].as_str().unwrap())
            .collect();
        roles.sort();
        assert_eq!(roles, vec!["Architect", "Tester"]);

        stop_swarm(State(state)).await.into_response();
    }

    #[tokio::test]
    async fn test_start_rejects_empty_roles() {
        let state = test_state();
        let body = Json(SwarmStartRequest {
            seed: None,
            roles: Some(vec![]),
            max_agents: None,
        });
        let response = start_swarm(State(state), Some(body))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_snapshot_reports_agents_consensus_and_last_event() {
        let swarm = SwarmState::new();
        let config = crate::config::Config::default();
        let engine = Arc::new(SwarmEngine::new(config.swarm.clone()));
        engine.init("Test swarm", &config).await.unwrap();
        swarm.attach(engine.clone()).await;
        engine.start().await.unwrap();

        engine
            .send_event(SwarmEvent::WorkCompleted {
                agent_id: "architect-1".to_string(),
                task: "Design API".to_string(),
                result: "API designed".to_string(),
            })
            .unwrap();

        // Let the engine's event loop process the work + the watcher record it.
        for _ in 0..50 {
            let snap = swarm_snapshot(&swarm, true).await;
            if snap["cycles"]["completed"] == 1 {
                assert_eq!(snap["consensus"]["entry_count"], 1);
                assert_eq!(snap["consensus"]["pending"], 1);
                let agents = snap["agents"].as_array().unwrap();
                let arch = agents
                    .iter()
                    .find(|a| a["id"] == "architect-1")
                    .unwrap();
                assert_eq!(arch["status"], "waiting_for_consensus");
                assert_eq!(arch["task"], Value::Null);
                let entry = &snap["consensus"]["entries"][0];
                assert_eq!(entry["author"], "architect-1");
                assert_eq!(entry["status"], "pending");

                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                let snap = swarm_snapshot(&swarm, true).await;
                // The watcher records engine events as they stream by; with no
                // live provider the runners' agent_error events are equally
                // valid "last activity". Verify the envelope shape, not the
                // exact race — work_completed itself is proven above by the
                // consensus entry, cycle count, and agent status.
                let last_type = snap["last_event"]["type"].as_str().unwrap();
                assert!(
                    [
                        "work_completed",
                        "review_requested",
                        "agent_error",
                        "heartbeat",
                    ]
                    .contains(&last_type),
                    "unexpected last_event type: {}",
                    last_type
                );
                assert!(snap["last_event"]["agent"].is_string());
                assert!(snap["last_event"]["at"].is_string());

                engine.stop().await.unwrap();
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("engine event loop did not process WorkCompleted");
    }

    #[tokio::test]
    async fn test_agent_view_working_task_exposed() {
        let agent = crate::swarm::SwarmAgent {
            id: "impl-1".to_string(),
            name: "Implementer 1".to_string(),
            role: crate::swarm::roles::AgentRole {
                name: "Implementer".to_string(),
                description: String::new(),
                responsibilities: vec![],
                system_prompt_addendum: String::new(),
            },
            status: AgentStatus::Working {
                task: "Implement core".to_string(),
                started_at: 0,
            },
            cycles_completed: 3,
            errors_count: 1,
            success_rate: 0.75,
            last_activity: Some("Working".to_string()),
            model: "test-model".to_string(),
        };
        let view = agent_view(&agent);
        assert_eq!(view["status"], "working");
        assert_eq!(view["task"], "Implement core");
        assert_eq!(view["model"], "test-model");
        assert_eq!(view["success_rate"], 0.75);
    }
}
