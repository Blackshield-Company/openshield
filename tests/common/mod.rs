//! Shared helpers for integration tests.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static ISOLATE: Once = Once::new();
// Plain counter, not a UUID: the PII scanner flags UUIDs in tool args.
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Point HOME / XDG dirs at a throwaway directory so tests never read or
/// write the developer's real OpenShield config, cache or memory DB.
pub fn isolate_home() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("openshield-test-home");
    ISOLATE.call_once(|| {
        // Start clean: the provider response cache lives here, and stale
        // cached replies from a previous run would bypass the mock server.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: called once, before any test spawns threads that read env.
        unsafe {
            std::env::set_var("HOME", &dir);
            std::env::set_var("USERPROFILE", &dir);
            std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
            std::env::set_var("XDG_CACHE_HOME", dir.join("cache"));
            std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        }
    });
    dir
}

/// A unique scratch directory inside the cargo target dir (which lives under
/// the crate root, so it is inside the sandbox's working directory).
pub fn scratch_dir(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Minimal mock of an OpenAI-compatible `/chat/completions` endpoint.
///
/// Each request pops the next canned assistant reply; the raw request bodies
/// are recorded so tests can assert on what the agent sent.
pub struct MockProvider {
    pub base_url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
}

impl MockProvider {
    pub async fn start(replies: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let queue = Arc::new(Mutex::new(VecDeque::from(replies)));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (q, r) = (queue.clone(), requests.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let (q, r) = (q.clone(), r.clone());
                tokio::spawn(async move {
                    // Read headers, then Content-Length bytes of body.
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let header_end = loop {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break p + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
                    let len: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    while buf.len() < header_end + len {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    r.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf[header_end..]).to_string());
                    let content = q.lock().unwrap().pop_front().unwrap_or_default();
                    let body = serde_json::json!({
                        "id": "mock-1",
                        "object": "chat.completion",
                        "created": 0,
                        "model": "mock-model",
                        "choices": [{
                            "index": 0,
                            "message": {"role": "assistant", "content": content},
                            "finish_reason": "stop"
                        }],
                        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                    })
                    .to_string();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self {
            base_url: format!("http://{addr}"),
            requests,
        }
    }

    pub fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
