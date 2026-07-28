//! OpenShark library crate root.
//!
//! Exposes the same modules as the `openshark` binary so embedders
//! (e.g. the Tauri Android backend) can host OpenShark in-process:
//! start the API server, run chat/agent turns, and manage config
//! without spawning an external binary.
//!
//! The TUI and other desktop-only pieces are excluded on Android.

/// The current version of OpenShark.
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
        .join("openshark")
        .join("openshark.log");
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

/// Start the OpenShark HTTP + WebSocket API server in-process.
///
/// This is the embedding entry point used by the Android app: the Tauri
/// backend calls it once at startup and the WebView talks to loopback.
///
/// * `config_dir` — directory holding `config.toml` (created if missing).
///   On Android this should be the app's private files dir.
/// * `addr` — bind address, e.g. `"127.0.0.1:1984"`.
///
/// Runs until the bind fails or the process exits; intended to be spawned
/// onto the host's tokio runtime.
#[cfg(feature = "web-api")]
pub async fn serve_in_process(
    config_dir: &std::path::Path,
    addr: &str,
) -> anyhow::Result<()> {
    let config = config::Config::load_from_dir(config_dir)?;
    let state = api::AppState {
        config: std::sync::Arc::new(config),
        running_tasks: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
    };
    api::serve(state, addr).await
}
