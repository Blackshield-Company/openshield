//! Human-in-the-loop approval bridge (M3, design doc §3.1 / §4).
//!
//! When the L4 security gate assesses a tool call High/Critical risk during a
//! WS chat turn AND at least one desktop/pocket client is connected to
//! `/ws/v1/chat`, the gate emits an `approval_req` event on the WS stream and
//! blocks the tool call until a human answers
//! `POST /api/v1/approvals/:id {decision}` — or the 120s timeout expires,
//! which defaults to DENY (design doc §6: "Approval timeout defaults to deny").
//!
//! When no WS client is connected, the gate is not installed by the WS layer
//! and the engine keeps its existing behavior — the serve path does not
//! weaken the gate relative to any other surface.
//!
//! Flow:
//!   engine RequireApproval(High/Critical) → ApprovalGate
//!     → ApprovalsState::request() → broadcast `approval_req` on /ws/v1/chat
//!     → blocks on a oneshot
//!   POST /api/v1/approvals/:id → ApprovalsState::resolve() → oneshot fired
//!     → broadcast `approval_res` → engine resumes (execute or deny)

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{broadcast, oneshot};

use super::{ApiError, AppState};
use crate::harness::{ApprovalGate, ApprovalRequest};

/// Default approval window: 120s, then the tool call is denied (doc §6).
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

/// Human decision on a pending approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Execute the tool call this once.
    Approve,
    /// Deny the tool call.
    Reject,
    /// Execute now and auto-approve future calls to the same tool.
    ApproveAlways,
}

impl ApprovalDecision {
    /// Wire string for the `approval_res` event (doc §4 envelope).
    fn as_str(&self) -> &'static str {
        match self {
            ApprovalDecision::Approve => "approved",
            ApprovalDecision::Reject => "rejected",
            ApprovalDecision::ApproveAlways => "approved_always",
        }
    }

    /// Whether the engine may execute the tool call.
    fn allows_execution(&self) -> bool {
        !matches!(self, ApprovalDecision::Reject)
    }
}

/// A pending approval request held by the state.
struct PendingApproval {
    id: String,
    session: String,
    risk: String,
    tool: String,
    args: serde_json::Value,
    reason: String,
    created_at: chrono::DateTime<chrono::Utc>,
    /// Fired by `resolve()` (or dropped on sender drop) with the decision.
    tx: oneshot::Sender<ApprovalDecision>,
}

/// Serializable view of a pending approval for the REST list endpoint.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PendingApprovalView {
    pub id: String,
    pub session: String,
    pub risk: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub reason: String,
    pub created_at: String,
}

/// Shared approval state hung off `AppState`.
pub struct ApprovalsState {
    pending: Mutex<HashMap<String, PendingApproval>>,
    /// Broadcast of ready-to-send WS envelopes (`approval_req` / `approval_res`),
    /// in the §4 shape `{type, session, data}`.
    events: broadcast::Sender<serde_json::Value>,
    /// Number of connected `/ws/v1/chat` clients. The human gate only engages
    /// when this is non-zero.
    ws_clients: AtomicUsize,
    /// Tools blessed by an `approve_always` decision.
    approved_always: Mutex<HashSet<String>>,
    /// Approval window before the request defaults to deny.
    timeout: Duration,
}

impl ApprovalsState {
    pub fn new() -> Self {
        Self::with_timeout(APPROVAL_TIMEOUT)
    }

    /// Test hook: build a state with a custom decision window.
    pub fn with_timeout(timeout: Duration) -> Self {
        // Capacity 64: approval traffic is tiny; lagged clients miss events.
        let (events, _) = broadcast::channel(64);
        Self {
            pending: Mutex::new(HashMap::new()),
            events,
            ws_clients: AtomicUsize::new(0),
            approved_always: Mutex::new(HashSet::new()),
            timeout,
        }
    }

    /// Subscribe to the approval event stream (WS clients forward these).
    pub fn subscribe(&self) -> broadcast::Receiver<serde_json::Value> {
        self.events.subscribe()
    }

    pub fn client_connected(&self) {
        self.ws_clients.fetch_add(1, Ordering::SeqCst);
    }

    pub fn client_disconnected(&self) {
        self.ws_clients.fetch_sub(1, Ordering::SeqCst);
    }

    fn connected_count(&self) -> usize {
        self.ws_clients.load(Ordering::SeqCst)
    }

    fn is_approved_always(&self, tool: &str) -> bool {
        self.approved_always
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(tool)
    }

    /// Snapshot of all pending requests, newest first.
    pub fn pending_list(&self) -> Vec<PendingApprovalView> {
        let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let mut views: Vec<PendingApprovalView> = pending
            .values()
            .map(|p| PendingApprovalView {
                id: p.id.clone(),
                session: p.session.clone(),
                risk: p.risk.clone(),
                tool: p.tool.clone(),
                args: p.args.clone(),
                reason: p.reason.clone(),
                created_at: p.created_at.to_rfc3339(),
            })
            .collect();
        views.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        views
    }

    /// Register a request, broadcast `approval_req`, and await the human
    /// decision. `true` = execute the tool call. Timeout (or abandoned
    /// receiver) resolves to `false` — deny by default.
    pub async fn request(&self, session: String, req: ApprovalRequest) -> bool {
        let id = uuid::Uuid::new_v4().to_string();
        let risk = format!("{:?}", req.risk).to_lowercase();
        let args = serde_json::from_str(&req.args)
            .unwrap_or_else(|_| serde_json::Value::String(req.args.clone()));
        let (tx, rx) = oneshot::channel();
        let expires = chrono::Utc::now() + chrono::Duration::from_std(self.timeout).unwrap();

        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.insert(
                id.clone(),
                PendingApproval {
                    id: id.clone(),
                    session: session.clone(),
                    risk: risk.clone(),
                    tool: req.tool_name.clone(),
                    args: args.clone(),
                    reason: req.reason.clone(),
                    created_at: chrono::Utc::now(),
                    tx,
                },
            );
        }

        tracing::info!(
            "approval requested: {} '{}' (risk {}) — awaiting human decision ({}s)",
            req.tool_name,
            req.args,
            risk,
            self.timeout.as_secs()
        );
        let _ = self.events.send(json!({
            "type": "approval_req",
            "session": session,
            "data": {
                "id": id,
                "risk": risk,
                "tool": req.tool_name,
                "args": args,
                "reason": req.reason,
                "expires": expires.to_rfc3339(),
            }
        }));

        let decision = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(d)) => d,
            // Timeout or resolver task died — deny (design doc §6).
            _ => ApprovalDecision::Reject,
        };

        // Whatever the outcome, the request is no longer pending.
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);

        if matches!(decision, ApprovalDecision::ApproveAlways) {
            self.approved_always
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(req.tool_name.clone());
        }

        let decision_str = decision.as_str();
        tracing::info!("approval {} resolved: {}", id, decision_str);
        let _ = self.events.send(json!({
            "type": "approval_res",
            "session": session,
            "data": { "id": id, "decision": decision_str }
        }));

        decision.allows_execution()
    }

    /// Resolve a pending request by id. Returns false when the id is unknown
    /// or was already resolved/timed out.
    pub fn resolve(&self, id: &str, decision: ApprovalDecision) -> bool {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        match pending {
            Some(p) => {
                // Dropping the sender would deny — send explicitly instead.
                let _ = p.tx.send(decision);
                true
            }
            None => false,
        }
    }
}

impl Default for ApprovalsState {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the async gate the harness engine awaits inside `execute_tool_call`.
///
/// Engagement rules (design doc §3.1/§4):
/// - Only High/Critical risk requests reach the human.
/// - Only when at least one `/ws/v1/chat` client is connected — otherwise the
///   engine's existing behavior applies unchanged (no weakening, no new
///   blocking surfaces).
/// - `approve_always` decisions short-circuit per tool.
pub fn make_gate(approvals: Arc<ApprovalsState>, session: String) -> ApprovalGate {
    Arc::new(move |req| {
        let approvals = approvals.clone();
        let session = session.clone();
        Box::pin(async move {
            use crate::security::RiskLevel;
            if req.risk < RiskLevel::High {
                return true;
            }
            if approvals.connected_count() == 0 {
                return true;
            }
            if approvals.is_approved_always(&req.tool_name) {
                return true;
            }
            approvals.request(session, req).await
        })
    })
}

/// Request body for POST /api/v1/approvals/:id.
#[derive(Debug, Deserialize)]
pub struct ApprovalResolveBody {
    pub decision: String,
}

fn parse_decision(raw: &str) -> Option<ApprovalDecision> {
    match raw {
        "approve" => Some(ApprovalDecision::Approve),
        "reject" => Some(ApprovalDecision::Reject),
        "approve_always" => Some(ApprovalDecision::ApproveAlways),
        _ => None,
    }
}

/// GET /api/v1/approvals — pending security-gate approvals (doc §3.1).
pub async fn list_approvals(State(state): State<AppState>) -> impl IntoResponse {
    let views = state.approvals.pending_list();
    Json(json!({
        "approvals": views,
        "pending": views.len(),
        "ws_clients": state.approvals.connected_count(),
    }))
    .into_response()
}

/// POST /api/v1/approvals/:id — approve / reject / approve-always a pending
/// request. 404 for unknown or already-resolved ids.
pub async fn resolve_approval(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Json<ApprovalResolveBody>,
) -> impl IntoResponse {
    let Some(decision) = parse_decision(&body.decision) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "decision must be approve | reject | approve_always".to_string(),
            }),
        )
            .into_response();
    };
    if state.approvals.resolve(&id, decision) {
        Json(json!({
            "ok": true,
            "id": id,
            "decision": decision.as_str(),
        }))
        .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("No pending approval: {}", id),
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_decision() {
        assert_eq!(parse_decision("approve"), Some(ApprovalDecision::Approve));
        assert_eq!(parse_decision("reject"), Some(ApprovalDecision::Reject));
        assert_eq!(
            parse_decision("approve_always"),
            Some(ApprovalDecision::ApproveAlways)
        );
        assert_eq!(parse_decision("yolo"), None);
    }

    #[tokio::test]
    async fn test_request_blocks_until_resolved() {
        let state = Arc::new(ApprovalsState::with_timeout(Duration::from_secs(5)));
        state.client_connected();
        let mut events = state.subscribe();

        let gate = make_gate(state.clone(), "sess-1".to_string());
        let handle = tokio::spawn(async move {
            gate(ApprovalRequest {
                tool_name: "terminal".to_string(),
                args: "curl http://example.com".to_string(),
                risk: crate::security::RiskLevel::High,
                reason: "Risk level 'High' exceeds auto-approve threshold".to_string(),
            })
            .await
        });

        // The approval_req event hits the broadcast stream.
        let ev = events.recv().await.unwrap();
        assert_eq!(ev["type"], "approval_req");
        assert_eq!(ev["session"], "sess-1");
        assert_eq!(ev["data"]["tool"], "terminal");
        assert_eq!(ev["data"]["risk"], "high");
        let req_id = ev["data"]["id"].as_str().unwrap().to_string();

        // Pending list shows it.
        let pending = state.pending_list();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, req_id);

        // Approve via the resolver; the gate unblocks with `true`.
        assert!(state.resolve(&req_id, ApprovalDecision::Approve));
        assert!(handle.await.unwrap());

        // approval_res broadcast + pending drained.
        let ev = events.recv().await.unwrap();
        assert_eq!(ev["type"], "approval_res");
        assert_eq!(ev["data"]["id"], req_id);
        assert_eq!(ev["data"]["decision"], "approved");
        assert!(state.pending_list().is_empty());
    }

    #[tokio::test]
    async fn test_timeout_defaults_to_deny() {
        let state = ApprovalsState::with_timeout(Duration::from_millis(50));
        state.client_connected();
        let gate = make_gate(Arc::new(state), "sess-2".to_string());
        let allowed = gate(ApprovalRequest {
            tool_name: "terminal".to_string(),
            args: "rm -rf /".to_string(),
            risk: crate::security::RiskLevel::Critical,
            reason: "Critical".to_string(),
        })
        .await;
        assert!(!allowed, "timeout must deny");
    }

    #[tokio::test]
    async fn test_gate_passthrough_without_ws_clients_or_lower_risk() {
        let state = Arc::new(ApprovalsState::with_timeout(Duration::from_secs(5)));
        // No WS clients connected → gate never blocks.
        let gate = make_gate(state.clone(), "sess-3".to_string());
        let allowed = gate(ApprovalRequest {
            tool_name: "terminal".to_string(),
            args: "rm -rf /".to_string(),
            risk: crate::security::RiskLevel::Critical,
            reason: "Critical".to_string(),
        })
        .await;
        assert!(allowed, "no WS client — existing behavior unchanged");

        // Medium risk → auto-passthrough even with clients connected.
        state.client_connected();
        let gate = make_gate(state.clone(), "sess-3".to_string());
        let allowed = gate(ApprovalRequest {
            tool_name: "git".to_string(),
            args: "git push".to_string(),
            risk: crate::security::RiskLevel::Medium,
            reason: "Medium".to_string(),
        })
        .await;
        assert!(allowed, "below High risk never reaches the human gate");
    }

    #[tokio::test]
    async fn test_approve_always_short_circuits() {
        let state = Arc::new(ApprovalsState::with_timeout(Duration::from_secs(5)));
        state.client_connected();
        let gate = make_gate(state.clone(), "sess-4".to_string());

        // First call blocks; bless the tool via approve_always.
        let gate2 = gate.clone();
        let handle = tokio::spawn(async move {
            gate2(ApprovalRequest {
                tool_name: "terminal".to_string(),
                args: "curl http://example.com".to_string(),
                risk: crate::security::RiskLevel::High,
                reason: "High".to_string(),
            })
            .await
        });
        // Wait for the pending entry to appear.
        for _ in 0..50 {
            if !state.pending_list().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let id = state.pending_list()[0].id.clone();
        assert!(state.resolve(&id, ApprovalDecision::ApproveAlways));
        assert!(handle.await.unwrap());

        // Second call to the same tool: no new pending entry, instant true.
        let allowed = gate(ApprovalRequest {
            tool_name: "terminal".to_string(),
            args: "curl http://example.com/2".to_string(),
            risk: crate::security::RiskLevel::High,
            reason: "High".to_string(),
        })
        .await;
        assert!(allowed);
        assert!(state.pending_list().is_empty());
    }
}
