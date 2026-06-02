//! MCP tool adapter — wraps MCP tools as OpenShark Tool trait objects.
//!
//! This allows MCP-discovered tools to be used seamlessly alongside
//! native OpenShark tools.

use anyhow::Result;
use serde_json::Value;

use crate::mcp::McpManager;
use crate::mcp::protocol::{CallToolResult, McpTool};

use super::Tool;

/// An adapter that wraps an MCP tool as an OpenShark Tool.
pub struct McpToolAdapter {
    tool: McpTool,
    #[allow(dead_code)]
    server_name: String,
    manager: std::sync::Arc<tokio::sync::Mutex<McpManager>>,
}

impl McpToolAdapter {
    pub fn new(
        tool: McpTool,
        server_name: String,
        manager: std::sync::Arc<tokio::sync::Mutex<McpManager>>,
    ) -> Self {
        Self {
            tool,
            server_name,
            manager,
        }
    }

    /// Build a description that includes the server name.
    #[allow(dead_code)]
    fn full_description(&self) -> String {
        let base = self.tool.description.as_deref().unwrap_or("MCP tool");
        format!("{} [via MCP server: {}]", base, self.server_name)
    }
}

impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.tool.name
    }

    fn description(&self) -> &str {
        // Return a static string — we can't return a reference to a local String.
        // The Tool trait's description() returns &str, so we store it in the tool.
        self.tool.description.as_deref().unwrap_or("MCP tool")
    }

    fn execute(&self, args: &str) -> Result<String> {
        // Parse arguments as JSON
        let arguments: Value = if args.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(args).unwrap_or_else(|_| {
                // If it's not valid JSON, treat it as a single "input" string
                let mut map = serde_json::Map::new();
                map.insert("input".to_string(), Value::String(args.to_string()));
                Value::Object(map)
            })
        };

        // We need to block on the async call since Tool::execute is sync.
        // SAFETY: Use a dedicated single-threaded runtime to avoid deadlocking
        // the main tokio runtime. block_in_place on the main runtime can deadlock
        // when all threads are blocked.
        let manager = self.manager.clone();
        let tool_name = self.tool.name.clone();

        let result = std::thread::scope(|s| {
            s.spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| anyhow::anyhow!("Failed to build MCP runtime: {}", e))?;
                rt.block_on(async move {
                    let manager = manager.lock().await;
                    manager.call_tool(&tool_name, arguments).await
                })
            })
            .join()
            .map_err(|e| anyhow::anyhow!("MCP tool thread panicked: {:?}", e))?
        });

        match result {
            Ok(call_result) => Ok(format_call_result(&call_result)),
            Err(e) => Err(e),
        }
    }
}

/// Format a CallToolResult into a human-readable string.
fn format_call_result(result: &CallToolResult) -> String {
    let mut output = String::new();

    for content in &result.content {
        match content {
            crate::mcp::protocol::ToolContent::Text { text } => {
                output.push_str(text);
                output.push('\n');
            }
            crate::mcp::protocol::ToolContent::Image { data, mime_type } => {
                output.push_str(&format!("[Image: {} ({} bytes)]\n", mime_type, data.len()));
            }
            crate::mcp::protocol::ToolContent::Resource { resource } => {
                output.push_str(&format!("[Resource: {}]\n", resource.uri));
                if let Some(text) = &resource.text {
                    output.push_str(text);
                    output.push('\n');
                }
            }
        }
    }

    if result.is_error == Some(true) {
        output.push_str("\n[Tool reported an error]\n");
    }

    output.trim().to_string()
}

/// Build OpenShark ToolDefinition schemas from MCP tools for LLM tool calling.
#[allow(dead_code)]
pub fn mcp_tool_to_definition(tool: &McpTool) -> super::ToolDef {
    super::ToolDef {
        name: tool.name.clone(),
        description: tool.description.clone().unwrap_or_default(),
        parameters: tool.input_schema.clone().unwrap_or_else(|| {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        }),
    }
}
