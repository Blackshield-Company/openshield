//! MCP transport layer — stdio and SSE implementations.
//!
//! The transport handles the raw byte stream: sending requests and receiving
//! responses/notifications. Higher-level protocol logic lives in `protocol.rs`.

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use super::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

/// A message received from the transport.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TransportMessage {
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    Error(String),
}

/// Unified transport enum — avoids dyn compatibility issues with async traits.
pub enum McpTransport {
    Stdio(StdioTransport),
    Sse(SseTransport),
}

impl McpTransport {
    pub async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<String> {
        match self {
            McpTransport::Stdio(t) => t.send_request(request).await,
            McpTransport::Sse(t) => t.send_request(request).await,
        }
    }

    pub async fn send_notification(&mut self, notification: &JsonRpcRequest) -> Result<()> {
        match self {
            McpTransport::Stdio(t) => t.send_notification(notification).await,
            McpTransport::Sse(t) => t.send_notification(notification).await,
        }
    }

    #[allow(dead_code)]
    pub fn start_message_stream(&mut self, tx: mpsc::Sender<TransportMessage>) -> Result<()> {
        match self {
            McpTransport::Stdio(t) => t.start_message_stream(tx),
            McpTransport::Sse(t) => t.start_message_stream(tx),
        }
    }

    pub async fn close(&mut self) -> Result<()> {
        match self {
            McpTransport::Stdio(t) => t.close().await,
            McpTransport::Sse(t) => t.close().await,
        }
    }
}

/// Stdio transport — spawns a subprocess and communicates over stdin/stdout.
pub struct StdioTransport {
    child: Child,
    stdin: ChildStdin,
    stdout_reader: BufReader<ChildStdout>,
}

impl StdioTransport {
    pub async fn new(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        for (key, val) in env {
            cmd.env(key, val);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server: {}", command))?;

        let stdin = child.stdin.take().context("Failed to open stdin")?;
        let stdout = child.stdout.take().context("Failed to open stdout")?;

        Ok(Self {
            child,
            stdin,
            stdout_reader: BufReader::new(stdout),
        })
    }
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<String>;
    async fn send_notification(&mut self, notification: &JsonRpcRequest) -> Result<()>;
    #[allow(dead_code)]
    fn start_message_stream(&mut self, tx: mpsc::Sender<TransportMessage>) -> Result<()>;
    async fn close(&mut self) -> Result<()>;
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<String> {
        let json = serde_json::to_string(request).context("Failed to serialize request")?;
        let line = format!("{}\n", json);

        self.stdin
            .write_all(line.as_bytes())
            .await
            .context("Failed to write to stdin")?;
        self.stdin.flush().await.context("Failed to flush stdin")?;

        // Read response line
        let mut response_line = String::new();
        self.stdout_reader
            .read_line(&mut response_line)
            .await
            .context("Failed to read response from stdout")?;

        Ok(response_line.trim().to_string())
    }

    async fn send_notification(&mut self, notification: &JsonRpcRequest) -> Result<()> {
        let json =
            serde_json::to_string(notification).context("Failed to serialize notification")?;
        let line = format!("{}\n", json);

        self.stdin
            .write_all(line.as_bytes())
            .await
            .context("Failed to write notification to stdin")?;
        self.stdin.flush().await.context("Failed to flush stdin")?;

        Ok(())
    }

    fn start_message_stream(&mut self, _tx: mpsc::Sender<TransportMessage>) -> Result<()> {
        // For stdio, the response is read inline in send_request.
        // Notifications could be read in a background task, but most MCP servers
        // don't send unsolicited notifications over stdio.
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        let _ = self.stdin.shutdown().await;
        let _ = self.child.wait().await;
        Ok(())
    }
}

/// SSE transport — connects to an HTTP endpoint and receives server-sent events.
pub struct SseTransport {
    client: reqwest::Client,
    url: String,
    headers: HashMap<String, String>,
}

impl SseTransport {
    pub fn new(url: String, headers: HashMap<String, String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            url,
            headers,
        }
    }
}

#[async_trait]
impl Transport for SseTransport {
    async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<String> {
        let mut builder = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(request);

        for (key, val) in &self.headers {
            builder = builder.header(key, val);
        }

        let response = builder
            .send()
            .await
            .with_context(|| format!("Failed to send request to {}", self.url))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!("HTTP error {}: {}", status, body);
        }

        Ok(body)
    }

    async fn send_notification(&mut self, notification: &JsonRpcRequest) -> Result<()> {
        let mut builder = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(notification);

        for (key, val) in &self.headers {
            builder = builder.header(key, val);
        }

        let response = builder
            .send()
            .await
            .with_context(|| format!("Failed to send notification to {}", self.url))?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("HTTP error: {}", body);
        }

        Ok(())
    }

    fn start_message_stream(&mut self, tx: mpsc::Sender<TransportMessage>) -> Result<()> {
        let client = self.client.clone();
        let url = self.url.clone();
        let headers = self.headers.clone();

        tokio::spawn(async move {
            let mut builder = client.get(&url).header("Accept", "text/event-stream");
            for (key, val) in &headers {
                builder = builder.header(key, val);
            }

            match builder.send().await {
                Ok(response) => {
                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                let text = String::from_utf8_lossy(&bytes);
                                buffer.push_str(&text);

                                // Process SSE events
                                while let Some(pos) = buffer.find("\n\n") {
                                    let event = buffer[..pos].to_string();
                                    buffer = buffer[pos + 2..].to_string();

                                    if let Some(data_line) =
                                        event.lines().find(|l| l.starts_with("data:"))
                                    {
                                        let data = data_line["data:".len()..].trim_start();
                                        if data == "[DONE]" {
                                            continue;
                                        }
                                        let _ = tx
                                            .send(TransportMessage::Error(data.to_string()))
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(TransportMessage::Error(format!("stream error: {}", e)))
                                    .await;
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx
                        .send(TransportMessage::Error(format!("connection error: {}", e)))
                        .await;
                }
            }
        });

        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        // SSE connections close when the client drops
        Ok(())
    }
}

/// Helper to parse a raw JSON string into a transport message.
pub fn parse_transport_message(raw: &str) -> Result<TransportMessage> {
    let value: Value = serde_json::from_str(raw).context("Failed to parse JSON")?;

    // Check if it's a notification (no id field)
    if value.get("id").is_none() {
        let notification: JsonRpcNotification =
            serde_json::from_value(value).context("Failed to parse notification")?;
        return Ok(TransportMessage::Notification(notification));
    }

    // It's a response
    let response: JsonRpcResponse =
        serde_json::from_value(value).context("Failed to parse response")?;
    Ok(TransportMessage::Response(response))
}
