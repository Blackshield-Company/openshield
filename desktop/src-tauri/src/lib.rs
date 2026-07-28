//! OpenShark Desktop — Tauri backend.
//!
//! The GUI does not link the openshark crate directly (it pulls in heavy
//! gateway deps like serenity/matrix-sdk). Instead we drive the installed
//! `openshark` CLI binary: capture one-shot output for list/status commands
//! and stream stdout line-by-line for chat/agent runs.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;

const DEFAULT_SERVER_PORT: u16 = 1984;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct BinaryInfo {
    found: bool,
    path: Option<String>,
    version: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StreamEvent {
    /// A line of stdout
    Line { text: String },
    /// A line of stderr
    Error { text: String },
    /// Process exited
    Done { code: Option<i32> },
}

#[derive(Serialize, Clone)]
pub struct CommandResult {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

#[derive(Serialize, Clone)]
pub struct ConfigInfo {
    path: String,
    content: String,
}

#[derive(Serialize, Clone)]
pub struct ServerStatus {
    running: bool,
    owned: bool,
    port: u16,
    version: Option<String>,
}

struct ServerState {
    /// None = we adopted an already-running server (never killed by us)
    child: Option<Child>,
    port: u16,
}

static SERVER: Mutex<Option<ServerState>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn openshark_bin() -> Result<PathBuf, String> {
    which::which("openshark").map_err(|_| {
        "openshark binary not found on PATH. Install it with `cargo install --path ../openshark`."
            .to_string()
    })
}

fn config_path() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|d| d.join("openshark").join("config.toml"))
        .ok_or_else(|| "could not resolve config directory".to_string())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Locate the openshark binary and report its version.
#[tauri::command]
fn binary_info() -> BinaryInfo {
    match which::which("openshark") {
        Ok(path) => {
            let version = Command::new(&path)
                .arg("--version")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string());
            BinaryInfo {
                found: true,
                path: Some(path.display().to_string()),
                version,
            }
        }
        Err(_) => BinaryInfo {
            found: false,
            path: None,
            version: None,
        },
    }
}

/// Run an openshark subcommand and capture all output. Good for
/// non-interactive one-shots: models, tools list, memory query, doctor, stats.
#[tauri::command]
async fn run_openshark(args: Vec<String>) -> Result<CommandResult, String> {
    let bin = openshark_bin()?;
    let output = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .args(&args)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .output()
    })
    .await
    .map_err(|e| format!("task join failed: {e}"))?
    .map_err(|e| format!("failed to spawn openshark: {e}"))?;

    Ok(CommandResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code(),
    })
}

/// Run an openshark subcommand and stream stdout/stderr lines over a Channel.
/// Used for chat and agent runs so the UI updates live.
#[tauri::command]
async fn stream_openshark(
    args: Vec<String>,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    let bin = openshark_bin()?;
    tokio::task::spawn_blocking(move || {
        let mut child = Command::new(bin)
            .args(&args)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn openshark: {e}"))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let out_chan = on_event.clone();

        let err_handle = std::thread::spawn(move || {
            if let Some(stderr) = stderr {
                for line in BufReader::new(stderr).lines() {
                    if let Ok(text) = line {
                        let _ = on_event.send(StreamEvent::Error { text });
                    }
                }
            }
        });

        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines() {
                if let Ok(text) = line {
                    let _ = out_chan.send(StreamEvent::Line { text });
                }
            }
        }

        let _ = err_handle.join();
        let status = child.wait().map_err(|e| format!("wait failed: {e}"))?;
        let _ = out_chan.send(StreamEvent::Done {
            code: status.code(),
        });
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("task join failed: {e}"))?
}

/// Read the openshark config file.
#[tauri::command]
fn read_config() -> Result<ConfigInfo, String> {
    let path = config_path()?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    Ok(ConfigInfo {
        path: path.display().to_string(),
        content,
    })
}

/// Write the openshark config file, backing up the previous version first.
#[tauri::command]
fn write_config(content: String) -> Result<ConfigInfo, String> {
    let path = config_path()?;
    if path.exists() {
        let backup = path.with_extension("toml.bak");
        std::fs::copy(&path, &backup)
            .map_err(|e| format!("failed to back up config: {e}"))?;
    }
    std::fs::write(&path, &content)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok(ConfigInfo {
        path: path.display().to_string(),
        content,
    })
}

// ---------------------------------------------------------------------------
// openshark serve lifecycle
// ---------------------------------------------------------------------------

/// Dependency-free health probe: GET /api/v1/health, extract version.
fn health_check(port: u16) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(1000)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(1000)))
        .ok()?;
    stream
        .write_all(
            b"GET /api/v1/health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        )
        .ok()?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).ok()?;
    let status_line = buf.lines().next()?;
    if !status_line.contains(" 200") {
        return None;
    }
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or("");
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("version")
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn server_kill() {
    if let Ok(mut guard) = SERVER.lock() {
        if let Some(state) = guard.take() {
            if let Some(mut child) = state.child {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// Start (or adopt) an `openshark serve` API server.
/// bind_lan=true binds 0.0.0.0 so devices on the LAN/Tailnet can connect
/// (e.g. the Android app). Default is loopback-only.
#[tauri::command]
async fn server_start(port: Option<u16>, bind_lan: Option<bool>) -> Result<ServerStatus, String> {
    let port = port.unwrap_or(DEFAULT_SERVER_PORT);
    let bind = if bind_lan.unwrap_or(false) {
        format!("0.0.0.0:{port}")
    } else {
        format!("127.0.0.1:{port}")
    };
    tokio::task::spawn_blocking(move || {
        // Already tracked and healthy?
        if let Ok(guard) = SERVER.lock() {
            if let Some(state) = &*guard {
                if let Some(version) = health_check(state.port) {
                    return Ok(ServerStatus {
                        running: true,
                        owned: state.child.is_some(),
                        port: state.port,
                        version: Some(version),
                    });
                }
            }
        }

        // Adopt an externally started server on the target port.
        if let Some(version) = health_check(port) {
            if let Ok(mut guard) = SERVER.lock() {
                *guard = Some(ServerState { child: None, port });
            }
            return Ok(ServerStatus {
                running: true,
                owned: false,
                port,
                version: Some(version),
            });
        }

        // Spawn our own.
        let bin = openshark_bin()?;
        let child = Command::new(bin)
            .args(["serve", "-a", &bind])
            .env("NO_COLOR", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn openshark serve: {e}"))?;

        if let Ok(mut guard) = SERVER.lock() {
            *guard = Some(ServerState {
                child: Some(child),
                port,
            });
        }

        let start = Instant::now();
        loop {
            if let Some(version) = health_check(port) {
                return Ok(ServerStatus {
                    running: true,
                    owned: true,
                    port,
                    version: Some(version),
                });
            }
            if start.elapsed() > Duration::from_secs(15) {
                server_kill();
                return Err("openshark serve did not become healthy within 15s".to_string());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    })
    .await
    .map_err(|e| format!("task join failed: {e}"))?
}

/// Stop the server if we own it.
#[tauri::command]
fn server_stop() -> ServerStatus {
    server_kill();
    ServerStatus {
        running: false,
        owned: false,
        port: DEFAULT_SERVER_PORT,
        version: None,
    }
}

/// Report server status without changing anything.
#[tauri::command]
fn server_status(port: Option<u16>) -> ServerStatus {
    let tracked = SERVER
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| (s.port, s.child.is_some())));
    let (port, owned) = match (tracked, port) {
        (Some((p, o)), _) => (p, o),
        (None, Some(p)) => (p, false),
        (None, None) => (DEFAULT_SERVER_PORT, false),
    };
    match health_check(port) {
        Some(version) => ServerStatus {
            running: true,
            owned,
            port,
            version: Some(version),
        },
        None => ServerStatus {
            running: false,
            owned,
            port,
            version: None,
        },
    }
}

// ---------------------------------------------------------------------------
// App setup
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            binary_info,
            run_openshark,
            stream_openshark,
            read_config,
            write_config,
            server_start,
            server_stop,
            server_status,
        ])
        .build(tauri::generate_context!())
        .expect("error while building openshark-desktop");

    app.run(|_handle, event| {
        if let tauri::RunEvent::Exit = event {
            server_kill();
        }
    });
}
