//! Async JSON-RPC transport layer for LSP over stdio.
//!
//! Uses tokio for fully async I/O with Content-Length framing,
//! background reader task, request/response routing via oneshot channels,
//! and broadcast notifications for multiple subscribers.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, broadcast, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

/// Async transport for JSON-RPC over stdio using Content-Length framing.
///
/// Spawns a background reader task that routes incoming messages:
/// - Responses (contain `"id"`) are dispatched to the oneshot channel
///   registered by the pending `send_request` call.
/// - Notifications (contain `"method"` but no `"id"`) are broadcast
///   to all subscribers via a `tokio::sync::broadcast` channel.
pub struct AsyncTransport {
    stdin: Arc<Mutex<ChildStdin>>,
    pending_responses: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    notification_tx: broadcast::Sender<Value>,
    reader_handle: Option<JoinHandle<()>>,
    next_id: AtomicI64,
}

impl AsyncTransport {
    /// Spawn the LSP server process and start the background reader task.
    ///
    /// Returns `(AsyncTransport, Child)` so the caller retains ownership of
    /// the child process for lifecycle management (e.g. killing on shutdown).
    pub async fn spawn(command: &str, args: &[&str]) -> Result<(Self, Child)> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to start LSP server: {command}"))?;

        let stdin = child
            .stdin
            .take()
            .context("Failed to acquire stdin of LSP server")?;
        let stdout = child
            .stdout
            .take()
            .context("Failed to acquire stdout of LSP server")?;

        let stdin = Arc::new(Mutex::new(stdin));
        let pending_responses: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (notification_tx, _) = broadcast::channel(256);

        // Spawn background reader task
        let reader_pending = pending_responses.clone();
        let reader_notify = notification_tx.clone();
        let reader_handle = tokio::spawn(async move {
            read_loop(BufReader::new(stdout), reader_pending, reader_notify).await;
        });

        let transport = AsyncTransport {
            stdin,
            pending_responses,
            notification_tx,
            reader_handle: Some(reader_handle),
            next_id: AtomicI64::new(0),
        };

        Ok((transport, child))
    }

    /// Send a JSON-RPC request and await the response.
    ///
    /// Allocates a new request ID, registers a oneshot channel in the pending
    /// map, writes the request to stdin, then awaits the response from the
    /// background reader task.
    pub async fn send_request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_responses.lock().await;
            pending.insert(id, tx);
        }

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        self.send_message(&request).await?;

        let response = rx
            .await
            .context("Reader task dropped before response arrived")?;

        // Check for JSON-RPC error
        if let Some(error) = response.get("error") {
            anyhow::bail!("LSP error: {error}");
        }

        // Return the "result" field if present, otherwise the full response
        if let Some(result) = response.get("result") {
            Ok(result.clone())
        } else {
            Ok(Value::Null)
        }
    }

    /// Send a JSON-RPC notification (fire-and-forget, no response expected).
    pub async fn send_notification(&self, method: &str, params: Value) -> Result<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_message(&notification).await
    }

    /// Subscribe to incoming LSP notifications.
    ///
    /// Each call returns a new receiver. Multiple subscribers are supported
    /// via the internal broadcast channel.
    pub fn notifications(&self) -> broadcast::Receiver<Value> {
        self.notification_tx.subscribe()
    }

    /// Shut down the transport.
    ///
    /// Aborts the background reader task. The caller is responsible for
    /// killing the child process (obtained from `spawn`).
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
            debug!("LSP transport reader task aborted");
        }
        Ok(())
    }

    // ---- Internal helpers ----

    /// Write a single JSON-RPC message with Content-Length framing.
    async fn send_message(&self, message: &Value) -> Result<()> {
        let body = message.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(header.as_bytes())
            .await
            .context("Failed to write header to LSP stdin")?;
        stdin
            .write_all(body.as_bytes())
            .await
            .context("Failed to write body to LSP stdin")?;
        stdin.flush().await.context("Failed to flush LSP stdin")?;

        debug!("Sent LSP message: {} bytes", body.len());
        Ok(())
    }
}

impl Drop for AsyncTransport {
    fn drop(&mut self) {
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }
    }
}

/// Background reader loop: reads Content-Length framed messages from the
/// LSP server's stdout and routes them to the appropriate channel.
async fn read_loop<R: tokio::io::AsyncBufRead + Unpin>(
    mut reader: R,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    notify_tx: broadcast::Sender<Value>,
) {
    loop {
        match read_one_message(&mut reader).await {
            Ok(Some(value)) => {
                route_message(value, &pending, &notify_tx).await;
            }
            Ok(None) => {
                // EOF — server closed stdout
                debug!("LSP server closed stdout, reader task exiting");
                break;
            }
            Err(e) => {
                warn!("LSP reader error: {e:#}");
                break;
            }
        }
    }

    // Clean up any pending requests that will never receive a response
    let mut map = pending.lock().await;
    for (_, sender) in map.drain() {
        // Dropping the sender without sending will cause the receiver
        // to return a RecvError, which send_request translates into an error.
        drop(sender);
    }
}

/// Route a parsed JSON-RPC message to the correct consumer.
async fn route_message(
    value: Value,
    pending: &Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    notify_tx: &broadcast::Sender<Value>,
) {
    if let Some(id) = value.get("id").and_then(|v| v.as_i64()) {
        // Response — deliver to the waiting send_request call
        let sender = {
            let mut map = pending.lock().await;
            map.remove(&id)
        };
        match sender {
            Some(tx) => {
                if tx.send(value).is_err() {
                    debug!("Dropped response for request {id} — receiver already closed");
                }
            }
            None => {
                warn!("Received response for unknown request id {id}");
            }
        }
    } else if value.get("method").is_some() {
        // Notification — broadcast to all subscribers
        debug!(
            "LSP notification: {}",
            value.get("method").and_then(|m| m.as_str()).unwrap_or("?")
        );
        // Lagging receivers are fine; we don't care about old notifications
        let _ = notify_tx.send(value);
    } else {
        warn!("Received unexpected JSON-RPC message (no id or method): {value}");
    }
}

/// Read exactly one Content-Length framed message from the buffered reader.
///
/// Returns `Ok(None)` on EOF.
async fn read_one_message<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    let mut header_line = String::new();

    // Read headers until the blank line separator
    loop {
        header_line.clear();
        let bytes_read = reader
            .read_line(&mut header_line)
            .await
            .context("Failed to read header from LSP stdout")?;

        if bytes_read == 0 {
            // EOF
            return Ok(None);
        }

        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            // Blank line — end of headers
            break;
        }

        if let Some(len_str) = trimmed.strip_prefix("Content-Length:")
            && let Ok(len) = len_str.trim().parse::<usize>()
        {
            content_length = Some(len);
        }
        // Silently ignore other headers (e.g. Content-Type)
    }

    let len = content_length.context("Missing Content-Length header in LSP message")?;

    // Read exactly `len` bytes of JSON body
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .await
        .context("Failed to read LSP message body")?;

    let body = String::from_utf8(buf).context("LSP message body is not valid UTF-8")?;

    debug!("Received LSP message: {len} bytes");

    let value: Value = serde_json::from_str(&body)
        .with_context(|| format!("Failed to parse LSP JSON message: {body}"))?;

    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Verify that read_one_message correctly parses Content-Length framed JSON.
    #[tokio::test]
    async fn test_read_one_message() {
        let payload = r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}"#;
        let framed = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);

        let mut reader = BufReader::new(Cursor::new(framed));
        let msg = read_one_message(&mut reader).await.unwrap().unwrap();

        assert_eq!(msg["id"], 1);
        assert!(msg["result"].is_object());
    }

    /// Verify EOF detection.
    #[tokio::test]
    async fn test_read_one_message_eof() {
        let mut reader = BufReader::new(Cursor::new(""));
        let result = read_one_message(&mut reader).await.unwrap();
        assert!(result.is_none());
    }

    /// Verify that route_message sends responses to pending oneshot channels.
    #[tokio::test]
    async fn test_route_response() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (notify_tx, _) = broadcast::channel(16);

        let (tx, rx) = oneshot::channel();
        pending.lock().await.insert(42, tx);

        let response = json!({"jsonrpc": "2.0", "id": 42, "result": "ok"});
        route_message(response, &pending, &notify_tx).await;

        let result = rx.await.unwrap();
        assert_eq!(result["result"], "ok");
    }

    /// Verify that route_message broadcasts notifications.
    #[tokio::test]
    async fn test_route_notification() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (notify_tx, mut rx) = broadcast::channel(16);

        let notification =
            json!({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"message": "hi"}});
        route_message(notification.clone(), &pending, &notify_tx).await;

        let received = rx.try_recv().unwrap();
        assert_eq!(received["method"], "window/logMessage");
    }
}
