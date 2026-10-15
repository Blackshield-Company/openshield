//! `openshield demo`: a browser web terminal running the OpenShield TUI
//! against the built-in mock provider. No API keys required.

pub mod mock;

use axum::{
    Router,
    extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
    response::{Html, IntoResponse},
    routing::get,
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const PAGE: &str = include_str!("terminal.html");

pub fn env_port(var: &str) -> Option<u16> {
    std::env::var(var).ok().and_then(|v| v.parse().ok())
}

/// Write an isolated config dir whose only provider is the mock.
pub fn write_demo_config(root: &Path, mock_base: &str) -> anyhow::Result<()> {
    let dir = root.join("config").join("openshield");
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(root.join("data"))?;
    std::fs::create_dir_all(root.join("workspace"))?;
    let mut cfg = crate::config::Config::default();
    cfg.providers.clear();
    cfg.providers.insert(
        "mock".to_string(),
        crate::config::ProviderConfig {
            base_url: mock_base.to_string(),
            api_key: "demo".to_string(),
            models: vec![crate::config::ModelConfig {
                name: mock::MODEL.to_string(),
                context_length: 32_768,
                cost_per_1k_input: 0.0,
                cost_per_1k_output: 0.0,
                capabilities: vec!["chat".to_string()],
            }],
            kind: crate::config::ProviderKind::OpenAiCompatible,
            headers: Default::default(),
            env_file: None,
        },
    );
    cfg.default_model = mock::MODEL.to_string();
    cfg.memory_db_path = root.join("data").join("memory.db");
    std::fs::write(dir.join("config.toml"), toml::to_string_pretty(&cfg)?)?;
    Ok(())
}

#[derive(Clone)]
struct DemoState {
    root: PathBuf,
}

pub async fn run(bind: Option<String>, port: Option<u16>) -> anyhow::Result<()> {
    let bind = bind
        .or_else(|| std::env::var("OPENSHIELD_DEMO_BIND").ok())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = port
        .or_else(|| env_port("OPENSHIELD_DEMO_PORT"))
        .unwrap_or(7681);
    let root = std::env::var("OPENSHIELD_DEMO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("openshield-demo"));

    let mock_addr = mock::spawn_ephemeral().await?;
    write_demo_config(&root, &format!("http://{}/v1", mock_addr))?;

    let state = DemoState { root: root.clone() };
    let app = Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/health", get(|| async { "ok" }))
        .route(
            "/ws",
            get(move |ws: WebSocketUpgrade| {
                let st = state.clone();
                async move { ws.on_upgrade(move |s| session(s, st)).into_response() }
            }),
        );
    let addr = format!("{}:{}", bind, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("🛡 OpenShield demo web terminal: http://{}", addr);
    println!(
        "   mock provider: http://{}/v1  demo dir: {}",
        mock_addr,
        root.display()
    );
    axum::serve(listener, app).await?;
    Ok(())
}

async fn session(mut ws: WebSocket, st: DemoState) {
    let pty = native_pty_system();
    let pair = match pty.openpty(PtySize {
        rows: 30,
        cols: 100,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            let _ = ws
                .send(WsMessage::Text(format!("pty error: {e}\r\n").into()))
                .await;
            return;
        }
    };
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("openshield"));
    let mut cmd = CommandBuilder::new(exe);
    cmd.env("XDG_CONFIG_HOME", st.root.join("config"));
    cmd.env("XDG_DATA_HOME", st.root.join("data"));
    cmd.env("TERM", "xterm-256color");
    cmd.cwd(st.root.join("workspace"));
    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            let _ = ws
                .send(WsMessage::Text(format!("spawn error: {e}\r\n").into()))
                .await;
            return;
        }
    };
    drop(pair.slave);
    let master = pair.master;
    let (Ok(mut reader), Ok(mut writer)) = (master.try_clone_reader(), master.take_writer()) else {
        let _ = child.kill();
        return;
    };

    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            out = out_rx.recv() => match out {
                Some(bytes) => {
                    if ws.send(WsMessage::Binary(bytes.into())).await.is_err() { break; }
                }
                None => break,
            },
            msg = ws.recv() => match msg {
                Some(Ok(WsMessage::Text(t))) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { continue };
                    match v["type"].as_str() {
                        Some("input") => {
                            if let Some(d) = v["data"].as_str() {
                                let _ = writer.write_all(d.as_bytes());
                                let _ = writer.flush();
                            }
                        }
                        Some("resize") => {
                            let cols = v["cols"].as_u64().unwrap_or(100) as u16;
                            let rows = v["rows"].as_u64().unwrap_or(30) as u16;
                            let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                        }
                        _ => {}
                    }
                }
                Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            }
        }
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    #[test]
    fn writes_loadable_config() {
        let dir = std::env::temp_dir().join(format!("osdemo-test-{}", std::process::id()));
        super::write_demo_config(&dir, "http://127.0.0.1:1/v1").unwrap();
        let cfg =
            crate::config::Config::load_from_dir(&dir.join("config").join("openshield")).unwrap();
        assert_eq!(cfg.default_model, super::mock::MODEL);
        assert_eq!(cfg.providers.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
