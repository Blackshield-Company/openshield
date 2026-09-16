//! Bearer-token auth for `openshield serve` (design doc §3.1 / §6).
//!
//! Local mode (loopback bind) auto-generates `desktop.token` (chmod 600) and
//! accepts it, but does not require it. LAN mode (non-loopback bind, which
//! requires `lan_mode = true` in `[desktop]`) enforces `Authorization: Bearer`
//! on every HTTP request — WebSocket upgrades included, since the upgrade is
//! itself an HTTP request that passes through the middleware before the
//! handshake.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Auth material attached to `AppState` by the `serve` command.
pub struct AuthState {
    /// Master desktop token (contents of the token file).
    pub token: String,
    /// Enforce the bearer token on every request. True when serving on a
    /// non-loopback bind (LAN mode); false for local loopback serving.
    pub enforce: bool,
    /// Short-lived pairing tokens → expiry (UTC). Issued by
    /// `POST /api/v1/pair`, accepted alongside the master token until expiry.
    pub pair_tokens: Mutex<HashMap<String, chrono::DateTime<chrono::Utc>>>,
}

impl AuthState {
    pub fn new(token: String, enforce: bool) -> Arc<Self> {
        Arc::new(Self {
            token,
            enforce,
            pair_tokens: Mutex::new(HashMap::new()),
        })
    }

    /// Check a presented bearer token against the master token and any
    /// unexpired pairing token. Expired pairing tokens are pruned lazily.
    fn accepts(&self, presented: &str) -> bool {
        if presented == self.token {
            return true;
        }
        let mut pairs = self.pair_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let now = chrono::Utc::now();
        pairs.retain(|_, expiry| *expiry > now);
        pairs.contains_key(presented)
    }
}

/// Read (generating if missing) the desktop bearer token file.
///
/// The token is 64 hex chars (two UUIDv4s, no hyphens). The file is created
/// with mode 600 on unix so only the owner can read it.
pub fn ensure_token_file(path: &str) -> anyhow::Result<String> {
    let expanded = shellexpand::tilde(path).to_string();
    let path = std::path::Path::new(&expanded);

    if let Ok(existing) = std::fs::read_to_string(path) {
        let token = existing.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    std::fs::write(path, format!("{}\n", token))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!("Generated desktop auth token at {}", path.display());
    Ok(token)
}

/// Axum middleware: enforce the bearer token when `AuthState::enforce` is set
/// (LAN mode). No-op for embedded loopback hosts (`auth` is None) and for
/// local-mode serving, matching design doc §3.1.
pub async fn require_bearer(
    State(state): State<crate::api::AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(auth) = &state.auth else {
        return next.run(req).await;
    };
    if !auth.enforce {
        return next.run(req).await;
    }

    let presented = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);

    match presented {
        Some(token) if auth.accepts(token) => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized: Bearer token required (LAN mode)" })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accepts_master_token() {
        let auth = AuthState::new("master".to_string(), true);
        assert!(auth.accepts("master"));
        assert!(!auth.accepts("wrong"));
    }

    #[test]
    fn test_pair_token_expiry() {
        let auth = AuthState::new("master".to_string(), true);
        auth.pair_tokens.lock().unwrap().insert(
            "pair1".to_string(),
            chrono::Utc::now() + chrono::Duration::seconds(60),
        );
        assert!(auth.accepts("pair1"));

        // Force expiry, then confirm rejection + pruning.
        *auth.pair_tokens.lock().unwrap().get_mut("pair1").unwrap() =
            chrono::Utc::now() - chrono::Duration::seconds(1);
        assert!(!auth.accepts("pair1"));
        assert!(auth.pair_tokens.lock().unwrap().is_empty());
    }
}
