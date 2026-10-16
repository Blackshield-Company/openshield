//! Bearer-token auth for `openshield serve`.
//!
//! Whenever `AppState::auth` is `Some`, every request must present the master
//! token or an unexpired pairing token. Loopback binds are not exempt.
//! Embedded hosts that leave `auth` as `None` (Android) stay open.
//!
//! Tokens come from `Authorization: Bearer <token>`. WebSocket upgrade paths
//! (`/ws/…`) may also pass `?token=` because browsers cannot set headers on
//! the handshake. Comparison is constant-time (`subtle`): the master token
//! and every unexpired pairing token are compared, with no hash-map lookup
//! and no early exit.
//!
//! `Host` must be loopback (`localhost`, `127.0.0.1`, `::1`), the specific
//! bind address, the machine LAN address when bound to a wildcard, or a name
//! in `[desktop] allowed_hosts`. This blocks DNS rebinding. CORS is an
//! origin allowlist (`allowed_origin`), not `CorsLayer::permissive()`.
//!
//! `POST /api/v1/pair` requires the master token (pairing tokens cannot mint
//! new ones), allows at most 5 requests per 60s, and stores at most 32
//! pairing tokens.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Request, State};
use axum::http::header::{self, HeaderMap};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use subtle::ConstantTimeEq;

const PAIR_RATE_LIMIT: usize = 5;
const PAIR_RATE_WINDOW: Duration = Duration::from_secs(60);
const PAIR_TOKEN_CAP: usize = 32;

/// Auth material attached to `AppState` by the `serve` command.
pub struct AuthState {
    /// Master desktop token (contents of the token file).
    pub token: String,
    /// `openshield serve` always sets this `true`. Enforcement follows
    /// `auth: Some`, including loopback; the flag is retained for callers
    /// and health reporting.
    pub enforce: bool,
    /// Short-lived pairing tokens in insertion order (oldest first) → expiry.
    /// Issued by `POST /api/v1/pair`, accepted alongside the master token
    /// until expiry. Capped at 32.
    pub pair_tokens: Mutex<VecDeque<(String, chrono::DateTime<chrono::Utc>)>>,
    /// Sliding window of accepted `POST /api/v1/pair` instants.
    pub pair_hits: Mutex<VecDeque<Instant>>,
}

impl AuthState {
    pub fn new(token: String, enforce: bool) -> Arc<Self> {
        Arc::new(Self {
            token,
            enforce,
            pair_tokens: Mutex::new(VecDeque::new()),
            pair_hits: Mutex::new(VecDeque::new()),
        })
    }

    /// Constant-time check of `presented` against the master token and every
    /// unexpired pairing token. Expired pairing tokens are pruned lazily.
    /// Never returns early on a match, and never looks the token up by hash.
    pub fn accepts(&self, presented: &str) -> bool {
        let mut ok = presented.as_bytes().ct_eq(self.token.as_bytes());
        let mut pairs = self.pair_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let now = chrono::Utc::now();
        pairs.retain(|(_, expiry)| *expiry > now);
        for (token, _) in pairs.iter() {
            ok |= token.as_bytes().ct_eq(presented.as_bytes());
        }
        bool::from(ok)
    }

    /// Constant-time master-token check. Pairing tokens do not match.
    pub fn is_master(&self, presented: &str) -> bool {
        bool::from(presented.as_bytes().ct_eq(self.token.as_bytes()))
    }

    /// Record a pairing attempt. `false` when 5 requests already landed in
    /// the last 60 seconds (the rejected attempt is not recorded).
    pub fn allow_pair(&self) -> bool {
        let mut hits = self.pair_hits.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        while hits
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= PAIR_RATE_WINDOW)
        {
            hits.pop_front();
        }
        if hits.len() >= PAIR_RATE_LIMIT {
            return false;
        }
        hits.push_back(now);
        true
    }

    /// Store a pairing token, dropping expired entries and the oldest live
    /// entries until at most 32 remain.
    pub fn store_pair_token(&self, token: String, expires: chrono::DateTime<chrono::Utc>) {
        let mut pairs = self.pair_tokens.lock().unwrap_or_else(|e| e.into_inner());
        let now = chrono::Utc::now();
        pairs.retain(|(_, expiry)| *expiry > now);
        while pairs.len() >= PAIR_TOKEN_CAP {
            pairs.pop_front();
        }
        pairs.push_back((token, expires));
    }
}

/// `Authorization: Bearer <token>`, or `None` when the header is absent or
/// not a bearer credential.
pub(crate) fn bearer_from_headers(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    if token.is_empty() { None } else { Some(token) }
}

fn query_token(query: &str) -> Option<&str> {
    for part in query.split('&') {
        if let Some(value) = part.strip_prefix("token=")
            && !value.is_empty()
        {
            return Some(value);
        }
    }
    None
}

fn presented_token(req: &Request) -> Option<String> {
    if let Some(token) = bearer_from_headers(req.headers()) {
        return Some(token.to_string());
    }
    if req.uri().path().starts_with("/ws/") {
        req.uri().query().and_then(query_token).map(str::to_string)
    } else {
        None
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized: Bearer token required" })),
    )
        .into_response()
}

/// Axum middleware: require a bearer token whenever `state.auth` is `Some`,
/// including loopback binds. No-op only when `auth` is `None` (embedded hosts).
pub async fn require_bearer(
    State(state): State<crate::api::AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(auth) = &state.auth else {
        return next.run(req).await;
    };
    match presented_token(&req) {
        Some(token) if auth.accepts(&token) => next.run(req).await,
        _ => unauthorized(),
    }
}

/// Strip a port and IPv6 brackets. IPs are canonicalized; names are lowercased.
pub fn normalize_host(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    let host = if let Some(rest) = raw.strip_prefix('[') {
        match rest.find(']') {
            Some(end) => rest[..end].to_string(),
            None => raw.to_string(),
        }
    } else if let Some((name, port)) = raw.rsplit_once(':') {
        if !name.is_empty()
            && !name.contains(':')
            && !port.is_empty()
            && port.bytes().all(|b| b.is_ascii_digit())
        {
            name.to_string()
        } else {
            raw.to_string()
        }
    } else {
        raw.to_string()
    };
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        ip.to_string()
    } else {
        host.to_ascii_lowercase()
    }
}

fn push_unique(names: &mut Vec<String>, name: String) {
    if !name.is_empty() && !names.iter().any(|n| n == &name) {
        names.push(name);
    }
}

fn bind_ip(bind: &str) -> Option<std::net::IpAddr> {
    let trimmed = bind.trim();
    let bare = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);
    bare.parse().ok()
}

fn allowed_host_names(desktop: &crate::config::DesktopConfig) -> Vec<String> {
    let mut names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    match bind_ip(&desktop.bind) {
        Some(ip) if ip.is_unspecified() => {
            push_unique(
                &mut names,
                normalize_host(&crate::api::handlers::pair_host(&desktop.bind)),
            );
            if let Some(lan) = crate::api::handlers::lan_ip() {
                push_unique(&mut names, normalize_host(&lan));
            }
        }
        Some(ip) => push_unique(&mut names, ip.to_string()),
        None if !desktop.bind.trim().is_empty() => {
            push_unique(&mut names, normalize_host(&desktop.bind));
        }
        None => {}
    }
    for extra in &desktop.allowed_hosts {
        push_unique(&mut names, normalize_host(extra));
    }
    names
}

/// Whether `raw` (a Host header or host:port) is on the serve allowlist.
pub fn host_allowed(raw: &str, desktop: &crate::config::DesktopConfig) -> bool {
    let presented = normalize_host(raw);
    if presented.is_empty() {
        return false;
    }
    allowed_host_names(desktop)
        .iter()
        .any(|name| name == &presented)
}

fn request_host(req: &Request) -> Option<String> {
    if req.headers().contains_key(header::HOST) {
        let raw = req
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        return Some(raw.to_string());
    }
    req.uri().host().map(str::to_string)
}

/// Reject requests whose `Host` (or URI host, if `Host` is absent) is not on
/// the allowlist. A missing host is allowed: DNS rebinding always sends one.
pub async fn require_allowed_host(
    State(state): State<crate::api::AppState>,
    req: Request,
    next: Next,
) -> Response {
    if request_host(&req).is_some_and(|raw| !host_allowed(&raw, &state.config.desktop)) {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            Json(json!({ "error": "misdirected request: Host not allowed" })),
        )
            .into_response();
    }
    next.run(req).await
}

/// CORS allowlist: Tauri origins, loopback HTTP with any port, and `extra`.
pub fn allowed_origin(origin: &str, extra: &[String]) -> bool {
    if extra.iter().any(|item| item == origin) {
        return true;
    }
    matches!(
        origin,
        "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost"
    ) || loopback_http_origin(origin)
}

fn loopback_http_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://") else {
        return false;
    };
    let port = if let Some(after) = rest.strip_prefix("[::1]") {
        after
    } else if let Some(after) = rest.strip_prefix("127.0.0.1") {
        after
    } else if let Some(after) = rest.strip_prefix("localhost") {
        after
    } else {
        return false;
    };
    port.is_empty()
        || (port.starts_with(':')
            && port.len() > 1
            && port[1..].bytes().all(|b| b.is_ascii_digit()))
}

/// Origin-restricted CORS. Registered outermost so preflight is answered
/// before auth or `Host` checks.
pub fn cors_layer(extra: Vec<String>) -> tower_http::cors::CorsLayer {
    tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::predicate(
            move |origin: &HeaderValue, _parts: &axum::http::request::Parts| {
                origin
                    .to_str()
                    .ok()
                    .is_some_and(|origin| allowed_origin(origin, &extra))
            },
        ))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

/// Read (generating if missing) the desktop bearer token file.
///
/// The token is 64 hex chars (two UUIDv4s, no hyphens). The file is created
/// with mode 600 on unix so only the owner can read it.
pub fn ensure_token_file(path: &str) -> anyhow::Result<String> {
    let path = crate::paths::resolve_user_path(path);

    if let Ok(existing) = std::fs::read_to_string(&path) {
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
    std::fs::write(&path, format!("{token}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!("Generated desktop auth token at {}", path.display());
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    fn app_state(token: &str, enforce: bool) -> crate::api::AppState {
        state_with(AuthState::new(token.to_string(), enforce))
    }

    fn state_with(auth: Arc<AuthState>) -> crate::api::AppState {
        crate::api::AppState {
            config: Arc::new(crate::config::Config::default()),
            running_tasks: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            config_dir: None,
            auth: Some(auth),
            approvals: Arc::new(crate::api::approvals::ApprovalsState::new()),
            swarm: Arc::new(crate::api::swarm::SwarmState::new()),
            started: std::time::Instant::now(),
        }
    }

    async fn status(
        app: &axum::Router,
        method: &str,
        uri: &str,
        token: Option<&str>,
        host: Option<&str>,
    ) -> StatusCode {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(host) = host {
            builder = builder.header(header::HOST, host);
        }
        let req = builder.body(Body::empty()).unwrap();
        app.clone().oneshot(req).await.unwrap().status()
    }

    #[test]
    fn test_accepts_constant_time() {
        let auth = AuthState::new("master".to_string(), true);
        let future = chrono::Utc::now() + chrono::Duration::seconds(60);
        auth.store_pair_token("pair-token".to_string(), future);
        auth.store_pair_token("other-pair".to_string(), future);

        assert!(auth.accepts("master"));
        assert!(auth.accepts("pair-token"));
        assert!(auth.accepts("other-pair"));
        assert!(!auth.accepts("wrong"));
        assert!(!auth.accepts("pair"));
        assert!(!auth.accepts("pair-token-extra"));
        assert!(!auth.accepts("pair-toke"));

        auth.pair_tokens.lock().unwrap().push_back((
            "expired".to_string(),
            chrono::Utc::now() - chrono::Duration::seconds(1),
        ));
        assert!(!auth.accepts("expired"));
        assert!(
            auth.pair_tokens
                .lock()
                .unwrap()
                .iter()
                .all(|(token, _)| token != "expired")
        );
    }

    #[test]
    fn test_pair_token_cap() {
        let auth = AuthState::new("master".to_string(), true);
        let future = chrono::Utc::now() + chrono::Duration::seconds(60);
        for i in 0..40 {
            auth.store_pair_token(format!("t{i}"), future);
        }
        let pairs = auth.pair_tokens.lock().unwrap();
        assert_eq!(pairs.len(), PAIR_TOKEN_CAP);
        assert!(pairs.iter().all(|(token, _)| token != "t0"));
        assert!(pairs.iter().any(|(token, _)| token == "t39"));
    }

    #[test]
    fn test_allowed_origin() {
        assert!(allowed_origin("tauri://localhost", &[]));
        assert!(allowed_origin("http://tauri.localhost", &[]));
        assert!(allowed_origin("https://tauri.localhost", &[]));
        assert!(allowed_origin("http://localhost:1420", &[]));
        assert!(allowed_origin("http://127.0.0.1:9310", &[]));
        assert!(allowed_origin("http://[::1]:8080", &[]));
        assert!(allowed_origin("http://localhost", &[]));
        assert!(!allowed_origin("https://evil.example", &[]));
        assert!(!allowed_origin("http://evil.example", &[]));
        assert!(!allowed_origin("http://localhost.evil.com:80", &[]));
        assert!(!allowed_origin("https://localhost:1420", &[]));
        let extra = vec!["https://app.example".to_string()];
        assert!(allowed_origin("https://app.example", &extra));
        assert!(!allowed_origin("https://other.example", &extra));
    }

    #[test]
    fn test_host_allowed() {
        let desktop = crate::config::DesktopConfig::default();
        assert!(host_allowed("localhost:9310", &desktop));
        assert!(host_allowed("127.0.0.1:9310", &desktop));
        assert!(host_allowed("[::1]:9310", &desktop));
        assert!(!host_allowed("evil.example", &desktop));
        assert!(!host_allowed("evil.example:9310", &desktop));

        let mut named = desktop.clone();
        named.allowed_hosts = vec!["desktop.local".to_string()];
        assert!(host_allowed("desktop.local:9310", &named));

        let mut specific = desktop.clone();
        specific.bind = "192.168.1.9".to_string();
        assert!(host_allowed("192.168.1.9", &specific));
        assert!(!host_allowed("192.168.1.10", &specific));
    }

    #[tokio::test]
    async fn test_loopback_requires_token() {
        let app = crate::api::build_router(app_state("master", false));
        let host = Some("127.0.0.1:9310");
        assert_eq!(
            status(&app, "GET", "/api/v1/health", None, host).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "GET", "/api/v1/health", Some("master"), host).await,
            StatusCode::OK
        );
        assert_eq!(
            status(&app, "GET", "/api/v1/health", Some("wrong"), host).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn test_ws_query_token_ignored_off_ws() {
        let app = crate::api::build_router(app_state("master", true));
        let host = Some("localhost:9310");
        assert_ne!(
            status(&app, "GET", "/ws/v1/chat?token=master", None, host).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "GET", "/ws/v1/chat?token=wrong", None, host).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "GET", "/api/v1/health?token=master", None, host).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn test_host_header() {
        let app = crate::api::build_router(app_state("master", true));
        assert_eq!(
            status(
                &app,
                "GET",
                "/api/v1/health",
                Some("master"),
                Some("localhost:9310")
            )
            .await,
            StatusCode::OK
        );
        let evil = status(
            &app,
            "GET",
            "/api/v1/health",
            Some("master"),
            Some("evil.example"),
        )
        .await;
        assert!(evil.is_client_error());
        assert_ne!(evil, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_preflight_evil_origin() {
        let app = crate::api::build_router(app_state("master", true));
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/health")
            .header(header::HOST, "localhost:9310")
            .header(header::ORIGIN, "https://evil.example")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );

        let ok = Request::builder()
            .method("GET")
            .uri("/api/v1/health")
            .header(header::HOST, "localhost:9310")
            .header(header::AUTHORIZATION, "Bearer master")
            .header(header::ORIGIN, "http://localhost:1420")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(ok).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("http://localhost:1420")
        );
    }

    #[tokio::test]
    async fn test_pair_requires_master_and_rate_limits() {
        let auth = AuthState::new("master".to_string(), true);
        auth.store_pair_token(
            "pairing".to_string(),
            chrono::Utc::now() + chrono::Duration::seconds(120),
        );
        let app = crate::api::build_router(state_with(auth));
        let host = Some("localhost:9310");
        assert_eq!(
            status(&app, "POST", "/api/v1/pair", None, host).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "POST", "/api/v1/pair", Some("pairing"), host).await,
            StatusCode::UNAUTHORIZED
        );

        let limited = crate::api::build_router(app_state("master", true));
        for i in 0..PAIR_RATE_LIMIT {
            assert_eq!(
                status(&limited, "POST", "/api/v1/pair", Some("master"), host).await,
                StatusCode::OK,
                "pair {i}"
            );
        }
        assert_eq!(
            status(&limited, "POST", "/api/v1/pair", Some("master"), host).await,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}
