//! MCP Server Mode — Act as an MCP (Model Context Protocol) server
//!
//! Other tools can call OpenShark as an MCP server via stdio JSON-RPC.
//! Implements: initialize, tools/list, tools/call

#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::io::{self, BufRead, Write};

#[derive(Debug, Clone, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub version: String,
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            name: "openshark".to_string(),
            version: crate::VERSION.to_string(),
        }
    }

    pub async fn run_stdio(self) -> Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        for line in stdin.lock().lines() {
            let line = line.context("Failed to read line from stdin")?;
            if line.trim().is_empty() {
                continue;
            }

            let response = self.handle_request(&line).await;
            let response_json = serde_json::to_string(&response)?;
            writeln!(stdout, "{}", response_json).context("Failed to write response")?;
            stdout.flush()?;
        }

        Ok(())
    }

    async fn handle_request(&self, request_json: &str) -> serde_json::Value {
        let request: JsonRpcRequest = match serde_json::from_str(request_json) {
            Ok(r) => r,
            Err(e) => {
                return json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    },
                    "id": null
                });
            }
        };

        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tool_call(request.params).await,
            _ => json!({
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", request.method)
                }
            }),
        };

        json!({
            "jsonrpc": "2.0",
            "result": result,
            "id": request.id
        })
    }

    fn handle_initialize(&self) -> serde_json::Value {
        json!({
            "name": self.name,
            "version": self.version,
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            }
        })
    }

    fn handle_tools_list(&self) -> serde_json::Value {
        let tools = vec![
            json!({
                "name": "terminal",
                "description": "Execute a shell command",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Shell command to run" }
                    },
                    "required": ["command"]
                }
            }),
            json!({
                "name": "read_file",
                "description": "Read a file",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path" }
                    },
                    "required": ["path"]
                }
            }),
            json!({
                "name": "write_file",
                "description": "Write to a file",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }
            }),
            json!({
                "name": "search_files",
                "description": "Search files with regex",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Regex pattern" },
                        "path": { "type": "string", "description": "Directory to search" }
                    },
                    "required": ["pattern"]
                }
            }),
            json!({
                "name": "git",
                "description": "Run git commands",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Git subcommand" }
                    },
                    "required": ["command"]
                }
            }),
            json!({
                "name": "repo_map",
                "description": "Build a code graph of the repository",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Root directory" }
                    }
                }
            }),
        ];

        json!({ "tools": tools })
    }

    async fn handle_tool_call(&self, params: Option<serde_json::Value>) -> serde_json::Value {
        let params = match params {
            Some(p) => p,
            None => {
                return json!({
                    "error": {
                        "code": -32602,
                        "message": "Missing params"
                    }
                });
            }
        };

        let tool_name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                return json!({
                    "error": {
                        "code": -32602,
                        "message": "Missing tool name"
                    }
                });
            }
        };

        let args = params.get("arguments").cloned().unwrap_or(json!({}));

        match tool_name {
            "terminal" => {
                let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let tool = crate::tools::terminal::TerminalTool;
                match crate::tools::Tool::execute(&tool, cmd) {
                    Ok(output) => json!({ "content": [{ "type": "text", "text": output }] }),
                    Err(e) => {
                        json!({ "isError": true, "content": [{ "type": "text", "text": e.to_string() }] })
                    }
                }
            }
            "read_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let tool = crate::tools::fs::FsTool;
                match crate::tools::Tool::execute(&tool, &format!("read {}", path)) {
                    Ok(content) => json!({ "content": [{ "type": "text", "text": content }] }),
                    Err(e) => {
                        json!({ "isError": true, "content": [{ "type": "text", "text": e.to_string() }] })
                    }
                }
            }
            "write_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let tool = crate::tools::fs::FsTool;
                match crate::tools::Tool::execute(&tool, &format!("write {} {}", path, content)) {
                    Ok(_) => {
                        json!({ "content": [{ "type": "text", "text": "File written successfully" }] })
                    }
                    Err(e) => {
                        json!({ "isError": true, "content": [{ "type": "text", "text": e.to_string() }] })
                    }
                }
            }
            "search_files" => {
                let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                let tool = crate::tools::search::SearchTool;
                match crate::tools::Tool::execute(&tool, &format!("{} {}", pattern, path)) {
                    Ok(results) => json!({ "content": [{ "type": "text", "text": results }] }),
                    Err(e) => {
                        json!({ "isError": true, "content": [{ "type": "text", "text": e.to_string() }] })
                    }
                }
            }
            "git" => {
                let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let tool = crate::tools::git::GitTool;
                match crate::tools::Tool::execute(&tool, cmd) {
                    Ok(output) => json!({ "content": [{ "type": "text", "text": output }] }),
                    Err(e) => {
                        json!({ "isError": true, "content": [{ "type": "text", "text": e.to_string() }] })
                    }
                }
            }
            "repo_map" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                match crate::repo_map::build_repo_map(path) {
                    Ok(map) => {
                        let formatted = crate::repo_map::format_repo_map_compact(&map);
                        json!({ "content": [{ "type": "text", "text": formatted }] })
                    }
                    Err(e) => {
                        json!({ "isError": true, "content": [{ "type": "text", "text": e.to_string() }] })
                    }
                }
            }
            _ => json!({
                "error": {
                    "code": -32602,
                    "message": format!("Unknown tool: {}", tool_name)
                }
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
    id: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_server_new() {
        let server = McpServer::new();
        assert_eq!(server.name, "openshark");
    }

    #[test]
    fn test_handle_initialize() {
        let server = McpServer::new();
        let result = server.handle_initialize();
        assert!(result.get("name").is_some());
        assert!(result.get("capabilities").is_some());
    }

    #[test]
    fn test_handle_tools_list() {
        let server = McpServer::new();
        let result = server.handle_tools_list();
        let tools = result.get("tools")
            .expect("tools list should contain tools key")
            .as_array()
            .expect("tools should be an array");
        assert!(!tools.is_empty());
    }
}
