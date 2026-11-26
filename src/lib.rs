//! OpenShield library crate root.
//!
//! Exposes the same modules as the `openshield` binary so embedders
//! (e.g. the Tauri Android backend) can host OpenShield in-process:
//! start the API server, run chat/agent turns, and manage config
//! without spawning an external binary.
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
pub mod modules;
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

#[cfg(feature = "web-api")]
pub mod demo;

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

/// File name of the bearer token used by [`serve_in_process`], inside the
/// embedder's `config_dir`.
#[cfg(feature = "web-api")]
pub const IN_PROCESS_TOKEN_FILE: &str = "desktop.token";

/// Path of the bearer token file [`serve_in_process`] reads (or creates).
///
/// The embedder reads this file and sends the token from its WebView as
/// `Authorization: Bearer <token>` (or `?token=` on `/ws/…` upgrades).
#[cfg(feature = "web-api")]
pub fn in_process_token_path(config_dir: &std::path::Path) -> std::path::PathBuf {
    config_dir.join(IN_PROCESS_TOKEN_FILE)
}

/// Load or create the bearer token for [`serve_in_process`].
///
/// Reads `config_dir/desktop.token`, creating `config_dir` and the token
/// (mode 0600) when missing. The embedder passes the value to its WebView.
#[cfg(feature = "web-api")]
pub fn embedded_token(config_dir: &std::path::Path) -> anyhow::Result<String> {
    std::fs::create_dir_all(config_dir)?;
    api::auth::ensure_token_file(&in_process_token_path(config_dir).to_string_lossy())
}

/// Build the `AppState` for an in-process server with bearer auth enforced.
///
/// Refuses an empty token and, unless `[desktop] lan_mode = true`, any
/// non-loopback bind address (same rule as `openshield serve`).
#[cfg(feature = "web-api")]
pub fn in_process_state(
    config_dir: &std::path::Path,
    addr: &str,
    token: String,
) -> anyhow::Result<api::AppState> {
    let token = token.trim().to_string();
    if token.is_empty() {
        anyhow::bail!("serve_in_process: refusing to start with an empty auth token");
    }
    let sock: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| anyhow::anyhow!("serve_in_process: invalid bind address {addr:?}: {e}"))?;
    let mut config = config::Config::load_from_dir(config_dir)?;
    if !sock.ip().is_loopback() && !config.desktop.lan_mode {
        anyhow::bail!(
            "serve_in_process: refusing to bind non-loopback {addr} without [desktop] lan_mode = true"
        );
    }
    config.desktop.bind = sock.ip().to_string();
    config.desktop.port = sock.port();
    let memory_db_path = config.memory_db_path.clone();
    Ok(api::AppState {
        config: std::sync::Arc::new(config),
        running_tasks: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        config_dir: Some(config_dir.to_path_buf()),
        auth: Some(api::auth::AuthState::new(token, true)),
        approvals: std::sync::Arc::new(
            api::approvals::ApprovalsState::new().with_memory_db(memory_db_path),
        ),
        swarm: std::sync::Arc::new(api::swarm::SwarmState::new()),
        started: std::time::Instant::now(),
    })
}

/// Start the OpenShield HTTP + WebSocket API server in-process.
///
/// This is the embedding entry point used by the Android app: the Tauri
/// backend calls it once at startup and the WebView talks to loopback.
///
/// * `config_dir` — directory holding `config.toml` (created if missing).
///   On Android this should be the app's private files dir.
/// * `addr` — bind address, e.g. `"127.0.0.1:1984"`.
///
/// Every REST and WebSocket request must present the bearer token stored at
/// [`in_process_token_path`] (generated with mode 0600 on first start), as
/// `Authorization: Bearer <token>` or, for `/ws/…` upgrades, `?token=`.
///
/// Runs until the bind fails or the process exits; intended to be spawned
/// onto the host's tokio runtime.
#[cfg(feature = "web-api")]
pub async fn serve_in_process(config_dir: &std::path::Path, addr: &str) -> anyhow::Result<()> {
    let token = embedded_token(config_dir)?;
    serve_in_process_with_token(config_dir, addr, token).await
}

/// Like [`serve_in_process`], with a token supplied by the embedder.
#[cfg(feature = "web-api")]
pub async fn serve_in_process_with_token(
    config_dir: &std::path::Path,
    addr: &str,
    token: String,
) -> anyhow::Result<()> {
    let state = in_process_state(config_dir, addr, token)?;
    api::serve(state, addr).await
}

#[cfg(all(test, feature = "web-api"))]
mod in_process_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use tower::ServiceExt;

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "openshield-inproc-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn status(app: &axum::Router, uri: &str, token: Option<&str>) -> StatusCode {
        let mut b = Request::builder()
            .method("GET")
            .uri(uri)
            .header(header::HOST, "127.0.0.1:1984");
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        app.clone()
            .oneshot(b.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[test]
    fn test_in_process_state_enforces_auth() {
        let dir = TempDir::new();
        let state = in_process_state(dir.path(), "127.0.0.1:1984", "tok".into()).unwrap();
        let auth = state.auth.as_ref().expect("auth must be Some");
        assert!(auth.accepts("tok"));
        assert!(!auth.accepts(""));
    }

    #[test]
    fn test_in_process_state_rejects_empty_token_and_lan_bind() {
        let dir = TempDir::new();
        assert!(in_process_state(dir.path(), "127.0.0.1:1984", "  ".into()).is_err());
        assert!(in_process_state(dir.path(), "0.0.0.0:1984", "tok".into()).is_err());
        assert!(in_process_state(dir.path(), "not-an-addr", "tok".into()).is_err());
    }

    #[tokio::test]
    async fn test_in_process_rest_and_ws_require_token() {
        let dir = TempDir::new();
        let state = in_process_state(dir.path(), "127.0.0.1:1984", "tok".into()).unwrap();
        let app = api::build_router(state);
        assert_eq!(
            status(&app, "/api/v1/health", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "/api/v1/health", Some("bad")).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "/api/v1/health", Some("tok")).await,
            StatusCode::OK
        );
        assert_eq!(
            status(&app, "/ws/v1/chat", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "/ws/v1/agent", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&app, "/ws/v1/chat?token=bad", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_ne!(
            status(&app, "/ws/v1/chat?token=tok", None).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn test_in_process_token_file_created_and_reused() {
        let dir = TempDir::new();
        let nested = dir.path().join("not-yet-created");
        let path = in_process_token_path(&nested);
        let a = embedded_token(&nested).unwrap();
        let b = embedded_token(&nested).unwrap();
        assert_eq!(a, std::fs::read_to_string(&path).unwrap().trim());
        assert!(a.len() >= 32);
        assert_eq!(a, b);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
