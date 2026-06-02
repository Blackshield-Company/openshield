//! Native MCP (Model Context Protocol) client for OpenShark.
//!
//! Provides stdio + SSE transport, JSON-RPC 2.0 framing, tool discovery,
//! and tool execution. Integrates with OpenShark's tool system.

pub mod protocol;
pub mod transport;

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::gateway::McpServerConfig;

use protocol::*;
use transport::{SseTransport, StdioTransport, TransportMessage, parse_transport_message};

/// An active MCP server connection.
pub struct McpConnection {
    pub name: String,
    transport: transport::McpTransport,
    server_capabilities: Option<ServerCapabilities>,
    tools: RwLock<Vec<McpTool>>,
    connected: bool,
}

impl McpConnection {
    /// Connect to an MCP server using the given config.
    pub async fn connect(config: &McpServerConfig) -> Result<Self> {
        info!("Connecting to MCP server: {}", config.name);

        let transport = match &config.transport {
            crate::gateway::McpTransport::Stdio { command, args, env } => {
                debug!("Spawning stdio MCP server: {} {:?}", command, args);
                transport::McpTransport::Stdio(StdioTransport::new(command, args, env).await?)
            }
            crate::gateway::McpTransport::Sse { url, headers } => {
                debug!("Connecting to SSE MCP server: {}", url);
                transport::McpTransport::Sse(SseTransport::new(url.clone(), headers.clone()))
            }
        };

        let mut conn = Self {
            name: config.name.clone(),
            transport,
            server_capabilities: None,
            tools: RwLock::new(Vec::new()),
            connected: false,
        };

        // Initialize handshake
        conn.initialize().await?;

        info!("MCP server '{}' connected successfully", config.name);
        Ok(conn)
    }

    /// Perform the MCP initialize handshake.
    async fn initialize(&mut self) -> Result<()> {
        let params = InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "openshark".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let request = JsonRpcRequest::new("initialize", Some(json!(params)));
        let response = self.send_request(&request).await?;

        if let Some(error) = response.error {
            anyhow::bail!("MCP initialize failed: {}", error);
        }

        let result: InitializeResult = serde_json::from_value(
            response
                .result
                .context("Initialize response missing result")?,
        )
        .context("Failed to parse initialize result")?;

        self.server_capabilities = Some(result.capabilities);

        // Send initialized notification
        let notification = JsonRpcRequest::notification("notifications/initialized", None);
        self.transport.send_notification(&notification).await?;

        self.connected = true;
        Ok(())
    }

    /// Send a JSON-RPC request and wait for the matching response.
    async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let raw = self.transport.send_request(request).await?;

        // Some stdio servers echo the request before the response; skip it
        if raw.contains(&format!("\"method\":\"{}\"", request.method))
            && !raw.contains("\"result\"")
        {
            // Read the next line which should be the actual response
            debug!("Skipping echoed request, reading next line for response");
        }

        let msg = parse_transport_message(&raw)
            .with_context(|| format!("Failed to parse MCP response: {}", raw))?;

        match msg {
            TransportMessage::Response(resp) => Ok(resp),
            TransportMessage::Notification(n) => {
                warn!("Received unexpected notification: {:?}", n);
                anyhow::bail!("Expected response, got notification: {}", n.method)
            }
            TransportMessage::Error(e) => {
                anyhow::bail!("Transport error: {}", e)
            }
        }
    }

    /// Discover available tools from the server.
    pub async fn discover_tools(&mut self) -> Result<Vec<McpTool>> {
        if !self.connected {
            anyhow::bail!("MCP connection not initialized");
        }

        let request = JsonRpcRequest::new("tools/list", None);
        let response = self.send_request(&request).await?;

        if let Some(error) = response.error {
            anyhow::bail!("tools/list failed: {}", error);
        }

        let result: ToolsListResult = serde_json::from_value(
            response
                .result
                .context("tools/list response missing result")?,
        )
        .context("Failed to parse tools/list result")?;

        let mut tools = self.tools.write().await;
        *tools = result.tools.clone();

        info!(
            "Discovered {} tools from MCP server '{}'",
            result.tools.len(),
            self.name
        );
        for tool in &result.tools {
            debug!("  - {}", tool.name);
        }

        Ok(result.tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult> {
        if !self.connected {
            anyhow::bail!("MCP connection not initialized");
        }

        let params = CallToolParams {
            name: name.to_string(),
            arguments,
        };

        let request = JsonRpcRequest::new("tools/call", Some(json!(params)));
        let response = self.send_request(&request).await?;

        if let Some(error) = response.error {
            anyhow::bail!("tools/call failed: {}", error);
        }

        let result: CallToolResult = serde_json::from_value(
            response
                .result
                .context("tools/call response missing result")?,
        )
        .context("Failed to parse tools/call result")?;

        Ok(result)
    }

    /// Get cached tools (without re-discovering).
    pub async fn get_tools(&self) -> Vec<McpTool> {
        self.tools.read().await.clone()
    }

    /// Check if the server supports tools.
    pub fn supports_tools(&self) -> bool {
        self.server_capabilities
            .as_ref()
            .and_then(|c| c.tools.as_ref())
            .is_some()
    }

    /// Close the connection gracefully.
    pub async fn close(&mut self) -> Result<()> {
        self.connected = false;
        self.transport.close().await
    }
}

/// Manager for multiple MCP server connections.
pub struct McpManager {
    connections: RwLock<HashMap<String, McpConnection>>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
        }
    }

    /// Connect to all configured MCP servers.
    pub async fn connect_all(&self, configs: &[McpServerConfig]) -> Result<()> {
        for config in configs {
            match McpConnection::connect(config).await {
                Ok(mut conn) => {
                    if conn.supports_tools()
                        && let Err(e) = conn.discover_tools().await {
                            warn!("Failed to discover tools for '{}': {}", config.name, e);
                        }
                    let mut connections = self.connections.write().await;
                    connections.insert(config.name.clone(), conn);
                }
                Err(e) => {
                    error!("Failed to connect to MCP server '{}': {}", config.name, e);
                }
            }
        }
        Ok(())
    }

    /// Get all tools from all connected servers.
    pub async fn all_tools(&self) -> Vec<(String, McpTool)> {
        let connections = self.connections.read().await;
        let mut all = Vec::new();
        for (server_name, conn) in connections.iter() {
            let tools = conn.get_tools().await;
            for tool in tools {
                all.push((server_name.clone(), tool));
            }
        }
        all
    }

    /// Call a tool by name. Tries all servers until one succeeds.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult> {
        let mut connections = self.connections.write().await;

        for (server_name, conn) in connections.iter_mut() {
            let tools = conn.get_tools().await;
            if tools.iter().any(|t| t.name == tool_name) {
                debug!("Calling tool '{}' on server '{}'", tool_name, server_name);
                return conn.call_tool(tool_name, arguments).await;
            }
        }

        anyhow::bail!("Tool '{}' not found in any connected MCP server", tool_name)
    }

    /// Check if a tool exists across all servers.
    #[allow(dead_code)]
    pub async fn has_tool(&self, tool_name: &str) -> bool {
        let connections = self.connections.read().await;
        for conn in connections.values() {
            let tools = conn.get_tools().await;
            if tools.iter().any(|t| t.name == tool_name) {
                return true;
            }
        }
        false
    }

    /// Disconnect all servers.
    pub async fn disconnect_all(&self) -> Result<()> {
        let mut connections = self.connections.write().await;
        for (name, conn) in connections.iter_mut() {
            if let Err(e) = conn.close().await {
                warn!("Error closing MCP connection '{}': {}", name, e);
            }
        }
        connections.clear();
        Ok(())
    }

    /// Get connection status summary.
    pub async fn status(&self) -> Vec<(String, bool, usize)> {
        let connections = self.connections.read().await;
        let mut status = Vec::new();
        for (name, conn) in connections.iter() {
            let tool_count = conn.get_tools().await.len();
            status.push((name.clone(), conn.connected, tool_count));
        }
        status
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}
