//! OpenShield library crate root.
//!
//! Exposes the same modules as the `openshield` binary so embedders
//! (e.g. the Tauri Android backend) can host OpenShield in-process:
//! start the API server, run chat/agent turns, and manage config
//! without spawning an external binary. The in-process server requires
//! the same bearer token as `openshield serve`; the host obtains it with
//! `embedded_token` and passes it to its WebView.
//!
//! The TUI and other desktop-only pieces are excluded on Android.

/// The current version of OpenShield.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod agent;
pub mod cache;
pub mod capabilities;
pub mod code_index;
pub mod config;
pub mod context_mode;
pub mod context_pinner;
pub mod diff;
pub mod doctor;
pub mod evolution;
pub mod gateway;
pub mod guardian;
pub mod harness;
pub mod headless;
pub mod image_utils;
pub mod integrations;
pub mod json_output;
pub mod linting;
pub mod lsp;
pub mod mcp;
pub mod mcp_server;
pub mod memory;
pub mod plugins;
pub mod providers;
pub mod repo_map;
pub mod router;
pub mod sandbox;
pub mod security;
pub mod self_correction;
pub mod self_improve;
pub mod session;
pub mod skills;
pub mod slash_commands;
pub mod swarm;
pub mod tools;
pub mod utils;
pub mod watch;

// TUI requires crossterm/arboard (desktop-only deps).
#[cfg(not(target_os = "android"))]
pub mod tui;

#[cfg(feature = "web-api")]
pub mod api;

/// Append a line to the persistent debug log (mirrors the bin's helper).
pub fn debug_log(msg: &str) {
    let path = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield")
        .join("openshield.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(f, "[{}] {}", chrono::Utc::now().to_rfc3339(), msg);
    }
}

/// Load or create the bearer token for an embedded (Pocket in-app) server.
///
/// Reads `config_dir/desktop.token`, creating it (mode 600) via
/// [`crate::api::auth::ensure_token_file`] when missing or empty. The value
/// is stable across calls. The host passes it to its WebView: HTTP requests
/// use `Authorization: Bearer`, and WebSocket upgrades may pass `?token=`.
#[cfg(feature = "web-api")]
pub fn embedded_token(config_dir: &std::path::Path) -> anyhow::Result<String> {
    let path = config_dir.join("desktop.token");
    crate::api::auth::ensure_token_file(&path.to_string_lossy())
}

/// In-process API state used by [`serve_in_process`].
///
/// Attaches [`crate::api::auth::AuthState`] with enforcement on, so every
/// request — including loopback — requires the bearer token. When `addr`
/// parses as a socket address, `desktop.bind` / `desktop.port` are aligned
/// with it (same as `openshield serve`) so the `Host` allowlist matches the
/// listen address. Loopback names stay allowed; CORS already allows the
/// Android WebView origins `http://tauri.localhost` and
/// `https://tauri.localhost`.
#[cfg(feature = "web-api")]
pub(crate) fn embedded_app_state(
    config_dir: &std::path::Path,
    addr: &str,
) -> anyhow::Result<api::AppState> {
    let mut config = config::Config::load_from_dir(config_dir)?;
    let token = embedded_token(config_dir)?;
    if let Ok(sock) = addr.parse::<std::net::SocketAddr>() {
        config.desktop.bind = sock.ip().to_string();
        config.desktop.port = sock.port();
    }
    Ok(api::AppState {
        config: std::sync::Arc::new(config),
        running_tasks: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        config_dir: Some(config_dir.to_path_buf()),
        auth: Some(api::auth::AuthState::new(token, true)),
        approvals: std::sync::Arc::new(api::approvals::ApprovalsState::new()),
        swarm: std::sync::Arc::new(api::swarm::SwarmState::new()),
        started: std::time::Instant::now(),
    })
}

/// Start the OpenShield HTTP + WebSocket API server in-process.
///
/// This is the embedding entry point used by the Android / Pocket app: the
/// Tauri backend calls it once at startup and the WebView talks to loopback.
/// Every request requires the bearer token from [`embedded_token`] (the host
/// passes it to the WebView). WebSocket upgrades may pass `?token=`.
/// `Host` must be loopback or the bind address. CORS allows the Android
/// WebView origins `http://tauri.localhost` and `https://tauri.localhost`.
///
/// * `config_dir` — directory holding `config.toml` and `desktop.token`
///   (created if missing). On Android this should be the app's private files dir.
/// * `addr` — bind address, e.g. `"127.0.0.1:1984"`.
///
/// Runs until the bind fails or the process exits; intended to be spawned
/// onto the host's tokio runtime.
#[cfg(feature = "web-api")]
pub async fn serve_in_process(config_dir: &std::path::Path, addr: &str) -> anyhow::Result<()> {
    let state = embedded_app_state(config_dir, addr)?;
    api::serve(state, addr).await
}

#[cfg(all(test, feature = "web-api"))]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::header;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openshield-embedded-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn embedded_token_is_stable_and_creates_file() {
        let dir = scratch_dir();
        let path = dir.join("desktop.token");
        assert!(!path.exists());

        let first = embedded_token(&dir).unwrap();
        assert!(!first.is_empty());
        assert!(path.is_file());
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(on_disk.trim(), first);

        let second = embedded_token(&dir).unwrap();
        assert_eq!(first, second);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn embedded_router_requires_token() {
        let dir = scratch_dir();
        let addr = "127.0.0.1:1984";
        let token = embedded_token(&dir).unwrap();
        let state = embedded_app_state(&dir, addr).unwrap();
        assert!(state.auth.as_ref().is_some_and(|auth| auth.enforce));
        assert_eq!(state.config.desktop.bind, "127.0.0.1");
        assert_eq!(state.config.desktop.port, 1984);
        let app = api::build_router(state);

        async fn status(app: &axum::Router, token: Option<&str>, host: &str) -> StatusCode {
            let mut builder = Request::builder()
                .method("GET")
                .uri("/api/v1/health")
                .header(header::HOST, host);
            if let Some(token) = token {
                builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            let req = builder.body(Body::empty()).unwrap();
            app.clone().oneshot(req).await.unwrap().status()
        }

        assert_eq!(
            status(&app, None, "127.0.0.1:1984").await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, Some(&token), "127.0.0.1:1984").await,
            StatusCode::OK
        );
        assert_eq!(
            status(&app, Some("wrong"), "127.0.0.1:1984").await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, Some(&token), "localhost:1984").await,
            StatusCode::OK
        );
        let evil = status(&app, Some(&token), "evil.example").await;
        assert!(evil.is_client_error());
        assert_ne!(evil, StatusCode::UNAUTHORIZED);

        for origin in ["http://tauri.localhost", "https://tauri.localhost"] {
            let req = Request::builder()
                .method("GET")
                .uri("/api/v1/health")
                .header(header::HOST, "127.0.0.1:1984")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::ORIGIN, origin)
                .body(Body::empty())
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(
                res.headers()
                    .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .and_then(|v| v.to_str().ok()),
                Some(origin)
            );
        }

        let preflight = Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/health")
            .header(header::HOST, "127.0.0.1:1984")
            .header(header::ORIGIN, "https://tauri.localhost")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(preflight).await.unwrap();
        assert_ne!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("https://tauri.localhost")
        );

        let ws = Request::builder()
            .method("GET")
            .uri(format!("/ws/v1/chat?token={token}"))
            .header(header::HOST, "127.0.0.1:1984")
            .body(Body::empty())
            .unwrap();
        assert_ne!(
            app.clone().oneshot(ws).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let query_ignored = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/health?token={token}"))
            .header(header::HOST, "127.0.0.1:1984")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(query_ignored).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
