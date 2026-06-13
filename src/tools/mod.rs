pub mod r#async;
pub mod checkpoint;
pub mod custom;
pub mod detection;
pub mod edit;
pub mod fs;
pub mod git;
pub mod lsp;
pub mod mcp;
pub mod refactor;
pub mod search;
pub mod terminal;
pub mod test_runner;

pub use r#async::AsyncToolExecutor;
pub use checkpoint::{CheckpointStack, restore_checkpoint, save_checkpoint};
pub use detection::{ToolBatch, ToolSuggestion, detect_tool_suggestions};
pub use git::GitTool;

use anyhow::Result;
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
/// Tool definition for schema-based tools (e.g., MCP tools).
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn execute(&self, args: &str) -> Result<String>;
}

/// Async tool trait for tools that need async I/O (LSP, network, etc.)
#[async_trait::async_trait]
pub trait AsyncTool: Send + Sync {
    fn name(&self) -> &str;
    #[allow(dead_code)]
    fn description(&self) -> &str;
    async fn execute_async(&self, args: &str) -> anyhow::Result<String>;
}

/// Global cache for MCP-discovered tools, populated after MCP initialization.
static MCP_TOOLS: Mutex<Vec<Arc<dyn Tool>>> = Mutex::new(Vec::new());

/// Global cache for plugin tools, populated when PluginRegistry loads.
static PLUGIN_TOOLS: Mutex<Vec<Arc<dyn Tool>>> = Mutex::new(Vec::new());

/// Register MCP tools into the global cache. Called after MCP discovery.
pub fn register_mcp_tools(tools: Vec<Arc<dyn Tool>>) {
    if let Ok(mut guard) = MCP_TOOLS.lock() {
        guard.clear();
        guard.extend(tools);
    }
}

/// Register plugin tools into the global cache. Called after plugin discovery.
pub fn register_plugin_tools(tools: Vec<Arc<dyn Tool>>) {
    if let Ok(mut guard) = PLUGIN_TOOLS.lock() {
        guard.clear();
        guard.extend(tools);
    }
}

/// Get all native + capability + MCP tools.
pub fn get_tools() -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(edit::EditTool),
        Arc::new(fs::FsTool),
        Arc::new(git::GitTool),
        Arc::new(lsp::LspTool),
        Arc::new(refactor::RefactorTool),
        Arc::new(search::SearchTool),
        Arc::new(search::GrepTool),
        Arc::new(terminal::TerminalTool),
        Arc::new(test_runner::TestTool),
    ];

    // Add all capability tools (web, media, memory, productivity, etc.)
    for cap_tool in crate::capabilities::get_capability_tools() {
        tools.push(cap_tool);
    }

    // Add MCP-discovered tools
    if let Ok(mcp) = MCP_TOOLS.lock() {
        for tool in mcp.iter() {
            tools.push(Arc::clone(tool));
        }
    }

    // Add custom user-defined tools
    for tool in crate::tools::custom::get_custom_tools() {
        tools.push(tool);
    }

    // Add plugin tools
    if let Ok(plugins) = PLUGIN_TOOLS.lock() {
        for tool in plugins.iter() {
            tools.push(Arc::clone(tool));
        }
    }
    tools
}

/// Convert all available tools to OpenAI-compatible tool definitions for function calling.
pub fn get_openai_tool_definitions() -> Vec<crate::providers::ToolDefinition> {
    use crate::providers::{ToolDefinition, ToolFunction};
    use serde_json::json;

    get_tools()
        .iter()
        .map(|tool| {
            let name = tool.name().to_string();
            let description = tool.description().to_string();

            // Build a simple parameter schema based on the tool name
            let parameters = match name.as_str() {
                "terminal" => json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        }
                    },
                    "required": ["command"]
                }),
                "fs" => json!({
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["read", "write", "list", "tree", "stat", "glob", "find", "cat"],
                            "description": "The filesystem operation to perform"
                        },
                        "path": {
                            "type": "string",
                            "description": "The file or directory path"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write (for write operation)"
                        }
                    },
                    "required": ["operation", "path"]
                }),
                "git" => json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The git subcommand to run (e.g., 'status', 'log', 'diff')"
                        },
                        "args": {
                            "type": "string",
                            "description": "Additional arguments for the git command"
                        }
                    },
                    "required": ["command"]
                }),
                "search" => json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query or pattern"
                        },
                        "path": {
                            "type": "string",
                            "description": "The directory to search in"
                        }
                    },
                    "required": ["query"]
                }),
                "grep" => json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "The regex pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "The file or directory to search in"
                        }
                    },
                    "required": ["pattern", "path"]
                }),
                "test" => json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The project directory to run tests in"
                        },
                        "framework": {
                            "type": "string",
                            "enum": ["auto", "cargo", "jest", "pytest", "go"],
                            "description": "The test framework to use (auto-detect if not specified)"
                        }
                    },
                    "required": ["path"]
                }),
                "edit" => json!({
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "The file path to edit"
                        },
                        "old_string": {
                            "type": "string",
                            "description": "The text to find and replace"
                        },
                        "new_string": {
                            "type": "string",
                            "description": "The replacement text"
                        }
                    },
                    "required": ["file", "old_string", "new_string"]
                }),
                "refactor" => json!({
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "The file path to refactor"
                        },
                        "operation": {
                            "type": "string",
                            "enum": ["extract_function", "rename", "inline", "reorder"],
                            "description": "The refactoring operation"
                        }
                    },
                    "required": ["file", "operation"]
                }),
                "lsp" => json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "enum": ["goto_definition", "find_references", "hover", "completion", "diagnostics"],
                            "description": "The LSP command to run"
                        },
                        "file": {
                            "type": "string",
                            "description": "The file path"
                        },
                        "line": {
                            "type": "integer",
                            "description": "The line number (1-based)"
                        },
                        "column": {
                            "type": "integer",
                            "description": "The column number (1-based)"
                        }
                    },
                    "required": ["command", "file", "line", "column"]
                }),
                _ => json!({
                    "type": "object",
                    "properties": {
                        "args": {
                            "type": "string",
                            "description": "Arguments for the tool"
                        }
                    },
                    "required": ["args"]
                }),
            };

            ToolDefinition {
                r#type: "function".to_string(),
                function: ToolFunction {
                    name,
                    description,
                    parameters,
                },
            }
        })
        .collect()
}

/// Get only native tools (no capabilities, no MCP).
pub fn get_native_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(edit::EditTool),
        Arc::new(fs::FsTool),
        Arc::new(git::GitTool),
        Arc::new(lsp::LspTool),
        Arc::new(refactor::RefactorTool),
        Arc::new(search::SearchTool),
        Arc::new(search::GrepTool),
        Arc::new(terminal::TerminalTool),
        Arc::new(test_runner::TestTool),
    ]
}

/// Get only capability tools.
pub fn get_capability_tools() -> Vec<Arc<dyn Tool>> {
    crate::capabilities::get_capability_tools()
}

/// Get all tool names and descriptions for system prompts.
#[allow(dead_code)]
pub fn get_all_tool_descriptions() -> Vec<(String, String)> {
    get_tools()
        .iter()
        .map(|t| (t.name().to_string(), t.description().to_string()))
        .collect()
}

/// Get all async-native tools (LSP, refactor).
pub fn get_async_tools() -> Vec<std::sync::Arc<dyn AsyncTool>> {
    let manager = crate::lsp::global_lsp_manager();
    vec![
        std::sync::Arc::new(lsp::LspAsyncTool::new(manager.clone())),
        std::sync::Arc::new(refactor::RefactorAsyncTool::new(manager)),
    ]
}

/// Find an async tool by name.
pub fn find_async_tool(name: &str) -> Option<std::sync::Arc<dyn AsyncTool>> {
    get_async_tools().into_iter().find(|t| t.name() == name)
}

pub fn find_tool(name: &str) -> Option<Arc<dyn Tool>> {
    get_tools().into_iter().find(|tool| tool.name() == name)
}

/// Normalize JSON-formatted tool arguments to the CLI-style strings that tools expect.
/// When the model uses native function calling, arguments come as JSON objects like
/// `{"query": "fn main", "path": "src"}`. This converts them to the space-separated
/// format each tool's `execute()` method expects.
pub fn normalize_tool_args(tool_name: &str, args: &str) -> String {
    let trimmed = args.trim();
    if !trimmed.starts_with('{') {
        return args.to_string();
    }

    let parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return args.to_string(),
    };

    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return args.to_string(),
    };

    let get_str = |key: &str| -> Option<String> {
        obj.get(key).and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else {
                v.as_i64().map(|n| n.to_string())
            }
        })
    };

    let result = match tool_name {
        "terminal" => get_str("command"),
        "fs" => {
            if let (Some(operation), Some(path)) = (get_str("operation"), get_str("path")) {
                let mut result = format!("{} {}", operation, path);
                if let Some(content) = get_str("content") {
                    result.push(' ');
                    result.push_str(&content);
                }
                Some(result)
            } else {
                None
            }
        }
        "search" => {
            if let Some(query) = get_str("query") {
                let mut result = query;
                if let Some(path) = get_str("path") {
                    result.push(' ');
                    result.push_str(&path);
                }
                Some(result)
            } else {
                None
            }
        }
        "grep" => {
            if let (Some(pattern), Some(path)) = (get_str("pattern"), get_str("path")) {
                Some(format!("{} {}", pattern, path))
            } else {
                None
            }
        }
        "git" => {
            if let Some(command) = get_str("command") {
                let mut result = command;
                if let Some(args_str) = get_str("args") {
                    result.push(' ');
                    result.push_str(&args_str);
                }
                Some(result)
            } else {
                None
            }
        }
        "test" => {
            if let Some(path) = get_str("path") {
                let mut result = path;
                if let Some(framework) = get_str("framework") {
                    result.push(' ');
                    result.push_str(&framework);
                }
                Some(result)
            } else {
                None
            }
        }
        "edit" => {
            if let (Some(file), Some(old_string), Some(new_string)) = (
                get_str("file"),
                get_str("old_string"),
                get_str("new_string"),
            ) {
                Some(format!("{} {} {}", file, old_string, new_string))
            } else {
                None
            }
        }
        "refactor" => {
            if let (Some(file), Some(operation)) = (get_str("file"), get_str("operation")) {
                Some(format!("{} {}", file, operation))
            } else {
                None
            }
        }
        "lsp" => {
            if let (Some(command), Some(file), Some(line), Some(column)) = (
                get_str("command"),
                get_str("file"),
                get_str("line"),
                get_str("column"),
            ) {
                Some(format!("{} {} {} {}", command, file, line, column))
            } else {
                None
            }
        }
        "memory" | "session_search" | "context_engine" => {
            if let Some(args_str) = get_str("args") {
                Some(args_str)
            } else if let Some(content) = get_str("content") {
                let target = get_str("target").unwrap_or_else(|| "memory".to_string());
                Some(format!("--add {} --target {}", content, target))
            } else {
                None
            }
        }
        "todo" | "cronjob" | "skills" => {
            if let Some(args_str) = get_str("args") {
                Some(args_str)
            } else {
                get_str("task").map(|task| format!("--add {}", task))
            }
        }
        _ => get_str("args"),
    };

    result.unwrap_or_else(|| args.to_string())
}

/// Find a tool by name and execute it with the given arguments,
/// automatically normalizing JSON-formatted args to CLI format.
pub fn execute_tool(name: &str, args: &str) -> Option<Result<String>> {
    let tool = find_tool(name)?;
    let normalized = normalize_tool_args(name, args);
    Some(tool.execute(&normalized))
}
