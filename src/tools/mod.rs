pub mod android;
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
        Arc::new(android::AndroidTool),
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
                "android" => json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The Android operation to perform. Format: '<category> <operation> <args>'. Categories: files (list/read/write/delete), sms (list/send), contacts (list), calendar (list), clipboard (get/set), camera (capture), location, battery, wifi, apps (list/open/screenshot/info), notifications, device (info/storage/display). Examples: 'files list /sdcard', 'sms list 10', 'clipboard get', 'device info'"
                        }
                    },
                    "required": ["command"]
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
                        },
                        "repo": {
                            "type": "string",
                            "description": "Path to the repository to run against. REQUIRED whenever the target repo is not the current working directory."
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
                        "operation": {
                            "type": "string",
                            "enum": ["read", "write", "replace", "patch"],
                            "description": "The edit operation: read (view file), write (create/overwrite file), replace (search/replace text), patch (multi-line replacement)"
                        },
                        "file": {
                            "type": "string",
                            "description": "The file path to edit"
                        },
                        "old_string": {
                            "type": "string",
                            "description": "For replace/patch: the exact text to find and replace. Must be empty for write operations."
                        },
                        "new_string": {
                            "type": "string",
                            "description": "For write/replace/patch: the new content or replacement text"
                        }
                    },
                    "required": ["operation", "file"]
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
                // Web & Search capabilities
                "web" | "web_search" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["search", "scrape"],
                            "description": "search = DuckDuckGo query, scrape = fetch a URL's text",
                            "default": "search"
                        },
                        "query": {
                            "type": "string",
                            "description": "The search query (for action=search)"
                        },
                        "url": {
                            "type": "string",
                            "description": "The URL to scrape (for action=scrape)"
                        }
                    },
                    "required": []
                }),
                "browser" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["navigate", "snapshot", "click", "type"],
                            "description": "navigate = open URL and read text, snapshot = screenshot PNG, click = click CSS selector, type = type text into CSS selector"
                        },
                        "url": {
                            "type": "string",
                            "description": "The URL (required for navigate, optional for snapshot)"
                        },
                        "selector": {
                            "type": "string",
                            "description": "CSS selector (for click/type)"
                        },
                        "text": {
                            "type": "string",
                            "description": "Text to type (for type action)"
                        }
                    },
                    "required": ["action"]
                }),
                "x_search" => json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The X/Twitter search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of posts to return",
                            "default": 10
                        }
                    },
                    "required": ["query"]
                }),
                // Media capabilities
                "vision" => json!({
                    "type": "object",
                    "properties": {
                        "image_path": {
                            "type": "string",
                            "description": "Path to the image file to analyze"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "What to look for in the image"
                        }
                    },
                    "required": ["image_path"]
                }),
                "image_gen" => json!({
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "The image generation prompt"
                        },
                        "aspect_ratio": {
                            "type": "string",
                            "enum": ["landscape", "portrait", "square"],
                            "description": "Aspect ratio of the generated image",
                            "default": "landscape"
                        }
                    },
                    "required": ["prompt"]
                }),
                "video" => json!({
                    "type": "object",
                    "properties": {
                        "video_path": {
                            "type": "string",
                            "description": "Path to the video file to analyze"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "What to analyze in the video"
                        }
                    },
                    "required": ["video_path"]
                }),
                "video_gen" => json!({
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "The video generation prompt"
                        },
                        "duration": {
                            "type": "integer",
                            "description": "Duration in seconds",
                            "default": 5
                        }
                    },
                    "required": ["prompt"]
                }),
                "tts" => json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The text to convert to speech"
                        },
                        "output_path": {
                            "type": "string",
                            "description": "Optional output file path"
                        }
                    },
                    "required": ["text"]
                }),
                // Memory & Context capabilities
                "memory" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["add", "search", "list"],
                            "description": "add = save a memory, search = find memories, list = show all"
                        },
                        "content": {
                            "type": "string",
                            "description": "The memory text to save (for action=add)"
                        },
                        "query": {
                            "type": "string",
                            "description": "Search terms (for action=search)"
                        },
                        "target": {
                            "type": "string",
                            "enum": ["user", "memory"],
                            "description": "user = facts about the user, memory = agent notes (for action=add)",
                            "default": "memory"
                        }
                    },
                    "required": ["action"]
                }),
                "session_search" => json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The session search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum sessions to return",
                            "default": 5
                        }
                    },
                    "required": ["query"]
                }),
                "context_engine" => json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The context query"
                        },
                        "target": {
                            "type": "string",
                            "enum": ["session", "project", "global"],
                            "description": "Which memory layer to query",
                            "default": "session"
                        }
                    },
                    "required": ["query"]
                }),
                // Productivity capabilities
                "todo" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list", "add", "complete", "remove"],
                            "description": "The todo action"
                        },
                        "task": {
                            "type": "string",
                            "description": "The task description (for add)"
                        },
                        "id": {
                            "type": "string",
                            "description": "Task ID (for complete/remove)"
                        }
                    },
                    "required": ["action"]
                }),
                "cronjob" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list", "create", "remove", "run"],
                            "description": "The cronjob action"
                        },
                        "schedule": {
                            "type": "string",
                            "description": "Cron schedule expression (for create)"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "The prompt or task to schedule (for create)"
                        },
                        "job_id": {
                            "type": "string",
                            "description": "Job ID (for remove/run)"
                        }
                    },
                    "required": ["action"]
                }),
                "skills" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list", "view", "create", "delete"],
                            "description": "list = all skills, view = read one skill, create = write a new skill, delete = remove a skill"
                        },
                        "name": {
                            "type": "string",
                            "description": "Skill name (for view/create/delete)"
                        },
                        "content": {
                            "type": "string",
                            "description": "Skill markdown content (for create)"
                        }
                    },
                    "required": ["action"]
                }),
                // Communication capabilities
                "messaging" => json!({
                    "type": "object",
                    "properties": {
                        "platform": {
                            "type": "string",
                            "enum": ["discord", "telegram", "slack", "matrix"],
                            "description": "The messaging platform"
                        },
                        "channel": {
                            "type": "string",
                            "description": "The channel or user to message"
                        },
                        "message": {
                            "type": "string",
                            "description": "The message content"
                        }
                    },
                    "required": ["platform", "channel", "message"]
                }),
                // Smart Home capabilities
                "homeassistant" | "home_assistant" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list", "toggle", "set"],
                            "description": "list = show devices, toggle = flip a device, set = set device state"
                        },
                        "entity": {
                            "type": "string",
                            "description": "Device/entity ID (for toggle/set)"
                        },
                        "state": {
                            "type": "string",
                            "description": "Desired state (for set)"
                        }
                    },
                    "required": ["action"]
                }),
                "spotify" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["play", "pause", "resume", "queue", "now-playing"],
                            "description": "play also handles search (free-text query). No next/previous support."
                        },
                        "query": {
                            "type": "string",
                            "description": "Track/search query (for play/queue)"
                        }
                    },
                    "required": ["action"]
                }),
                // Platform capabilities
                "yuanbao" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["query", "send"],
                            "description": "The Yuanbao action"
                        },
                        "group": {
                            "type": "string",
                            "description": "The group name or ID"
                        },
                        "message": {
                            "type": "string",
                            "description": "The message to send"
                        }
                    },
                    "required": ["action"]
                }),
                "computer_use" => json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["click", "type", "screenshot", "scroll", "key"],
                            "description": "The computer action"
                        },
                        "x": {
                            "type": "integer",
                            "description": "X coordinate (for click)"
                        },
                        "y": {
                            "type": "integer",
                            "description": "Y coordinate (for click)"
                        },
                        "text": {
                            "type": "string",
                            "description": "Text to type (for type action)"
                        }
                    },
                    "required": ["action"]
                }),
                // Agentic capabilities
                "moa" => json!({
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "The task for the Mixture of Agents"
                        },
                        "agents": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "List of agent roles to use",
                            "default": ["architect", "implementer", "reviewer"]
                        }
                    },
                    "required": ["task"]
                }),
                "delegation" => json!({
                    "type": "object",
                    "properties": {
                        "agent": {
                            "type": "string",
                            "enum": ["claw", "opencode", "claude", "codex"],
                            "description": "The external agent to delegate to"
                        },
                        "task": {
                            "type": "string",
                            "description": "The task to delegate"
                        }
                    },
                    "required": ["agent", "task"]
                }),
                "clarify" => json!({
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "The clarifying question to ask the user"
                        }
                    },
                    "required": ["question"]
                }),
                // Execution capabilities
                "code_execution" => json!({
                    "type": "object",
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "The Python code to execute"
                        },
                        "language": {
                            "type": "string",
                            "enum": ["python"],
                            "description": "Only python is supported; safe to omit",
                            "default": "python"
                        },
                        "timeout": {
                            "type": "integer",
                            "description": "Timeout in seconds",
                            "default": 30
                        }
                    },
                    "required": ["code"]
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
        Arc::new(android::AndroidTool),
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
                let mut result = String::new();
                // Repo targeting: {"command":"status","repo":"/path/to/repo"}
                if let Some(repo) = get_str("repo").or_else(|| get_str("path")) {
                    result.push_str(&format!("--repo {} ", repo));
                }
                result.push_str(&command);
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
                let mut result = format!("run {}", path);
                if let Some(framework) = get_str("framework") {
                    result.push(' ');
                    result.push_str(&framework);
                }
                Some(result)
            } else {
                None
            }
        }
        "edit" => (|| {
            // Accepts both {"file","old_string","new_string"} (implies replace)
            // and {"operation":"read|write|replace","file"/"path",...}.
            // Emits the exact CLI syntax EditTool::execute expects,
            // including the " ||| " delimiter for replace.
            let file = get_str("file").or_else(|| get_str("path"))?;
            let operation = get_str("operation").unwrap_or_else(|| {
                if get_str("old_string").is_some() {
                    "replace".to_string()
                } else if get_str("content").is_some() || get_str("new_string").is_some() {
                    "write".to_string()
                } else {
                    "read".to_string()
                }
            });
            match operation.as_str() {
                "read" => Some(format!("read {}", file)),
                "write" => {
                    let content = get_str("content").or_else(|| get_str("new_string"))?;
                    Some(format!("write {} {}", file, content))
                }
                "replace" | "patch" => {
                    let old = get_str("old_string").or_else(|| get_str("old_lines"))?;
                    let new = get_str("new_string")
                        .or_else(|| get_str("content"))
                        .or_else(|| get_str("new_lines"))?;
                    Some(format!("{} {} {} ||| {}", operation, file, old, new))
                }
                _ => None,
            }
        })(),
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
        "memory" => (|| {
            if let Some(args_str) = get_str("args") {
                return Some(args_str);
            }
            let action = get_str("action").unwrap_or_else(|| {
                if get_str("content").is_some() {
                    "add".to_string()
                } else if get_str("query").is_some() {
                    // Ambiguous: memory schema says query = "search query OR content
                    // to add". K3 uses query for both; default to add when the
                    // content is long/sentence-like, else search.
                    "add".to_string()
                } else {
                    "list".to_string()
                }
            });
            match action.as_str() {
                "add" | "save" => {
                    let content = get_str("content")
                        .or_else(|| get_str("query"))
                        .or_else(|| get_str("text"))?;
                    let target = get_str("target").unwrap_or_else(|| "memory".to_string());
                    Some(format!("--add {} --target {}", content, target))
                }
                "search" | "find" | "semantic" => {
                    let q = get_str("query").or_else(|| get_str("content"))?;
                    Some(format!("--search {}", q))
                }
                "list" => Some("--list".to_string()),
                _ => None,
            }
        })(),
        "session_search" => (|| {
            if let Some(args_str) = get_str("args") {
                return Some(args_str);
            }
            let query = get_str("query")?;
            let mut result = query;
            if let Some(limit) = get_str("limit") {
                result.push_str(&format!(" --limit {}", limit));
            }
            Some(result)
        })(),
        "context_engine" => (|| {
            if let Some(args_str) = get_str("args") {
                return Some(args_str);
            }
            let query = get_str("query")?;
            let mut result = query;
            if let Some(skill) = get_str("inject").or_else(|| get_str("skill")) {
                result.push_str(&format!(" --inject {}", skill));
            }
            Some(result)
        })(),
        "todo" | "cronjob" => {
            if let Some(args_str) = get_str("args") {
                Some(args_str)
            } else {
                get_str("task").map(|task| format!("--add {}", task))
            }
        }
        "skills" => (|| {
            if let Some(args_str) = get_str("args") {
                return Some(args_str);
            }
            let action = get_str("action").unwrap_or_else(|| "list".to_string());
            match action.as_str() {
                "list" | "reload" => Some("--list".to_string()),
                "view" | "get" | "show" | "trigger" => {
                    let name = get_str("name")
                        .or_else(|| get_str("skill"))
                        .or_else(|| get_str("query"))?;
                    Some(format!("--view {}", name))
                }
                "create" | "add" => {
                    let name = get_str("name")?;
                    let content = get_str("content").unwrap_or_default();
                    Some(format!("--create {} {}", name, content))
                }
                "delete" | "remove" => {
                    let name = get_str("name")?;
                    Some(format!("--delete {}", name))
                }
                _ => None,
            }
        })(),
        // Web & Search capabilities
        "web" | "web_search" => (|| {
            // Scrape mode: explicit scrape action/url. Otherwise plain query —
            // NOTE: WebSearchTool treats the entire arg string as the query,
            // so never append flags here or they pollute the search terms.
            let action = get_str("action").unwrap_or_else(|| "search".to_string());
            if action == "scrape" {
                let url = get_str("url").or_else(|| get_str("query"))?;
                return Some(format!("--scrape {}", url));
            }
            get_str("query")
        })(),
        "browser" => (|| {
            let action = get_str("action").unwrap_or_else(|| {
                if get_str("url").is_some() {
                    "navigate".to_string()
                } else {
                    "snapshot".to_string()
                }
            });
            match action.as_str() {
                "navigate" | "visit" | "goto" | "open" | "extract" => {
                    let url = get_str("url")?;
                    Some(format!("--navigate {}", url))
                }
                "snapshot" | "screenshot" => {
                    let url = get_str("url").unwrap_or_default();
                    Some(format!("--snapshot {}", url).trim().to_string())
                }
                "click" => {
                    let sel = get_str("selector")?;
                    Some(format!("--click {}", sel))
                }
                "type" => {
                    let sel = get_str("selector")?;
                    let text = get_str("text")?;
                    Some(format!("--type {} {}", sel, text))
                }
                _ => None,
            }
        })(),
        "x_search" => (|| {
            let query = get_str("query")?;
            let mut result = query;
            if let Some(from) = get_str("from_date").or_else(|| get_str("from-date")) {
                result.push_str(&format!(" --from-date {}", from));
            }
            if let Some(to) = get_str("to_date").or_else(|| get_str("to-date")) {
                result.push_str(&format!(" --to-date {}", to));
            }
            Some(result)
        })(),
        // Media capabilities
        "vision" => {
            if let Some(image_path) = get_str("image_path") {
                let mut result = image_path;
                if let Some(prompt) = get_str("prompt") {
                    result.push_str(&format!(" {}", prompt));
                }
                Some(result)
            } else {
                None
            }
        }
        "image_gen" => {
            if let Some(prompt) = get_str("prompt") {
                let mut result = prompt;
                if let Some(ratio) = get_str("aspect_ratio") {
                    result.push_str(&format!(" --ratio {}", ratio));
                }
                Some(result)
            } else {
                None
            }
        }
        "video" => {
            if let Some(video_path) = get_str("video_path") {
                let mut result = video_path;
                if let Some(prompt) = get_str("prompt") {
                    result.push_str(&format!(" {}", prompt));
                }
                Some(result)
            } else {
                None
            }
        }
        "video_gen" => {
            if let Some(prompt) = get_str("prompt") {
                let mut result = prompt;
                if let Some(duration) = get_str("duration") {
                    result.push_str(&format!(" --duration {}", duration));
                }
                Some(result)
            } else {
                None
            }
        }
        "tts" => {
            if let Some(text) = get_str("text") {
                let mut result = text;
                if let Some(path) = get_str("output_path") {
                    result.push_str(&format!(" --output {}", path));
                }
                Some(result)
            } else {
                None
            }
        }
        // Communication capabilities
        "messaging" => {
            if let (Some(platform), Some(channel), Some(message)) = (
                get_str("platform"),
                get_str("channel"),
                get_str("message"),
            ) {
                Some(format!("{} {} {}", platform, channel, message))
            } else {
                None
            }
        }
        // Smart Home capabilities
        "homeassistant" | "home_assistant" => (|| {
            if let Some(args_str) = get_str("args") {
                return Some(args_str);
            }
            let action = get_str("action").unwrap_or_else(|| "list".to_string());
            match action.as_str() {
                "list" | "status" => Some("--list".to_string()),
                "toggle" | "turn_on" | "turn_off" => get_str("entity")
                    .or_else(|| get_str("device_id"))
                    .map(|e| format!("--toggle {}", e)),
                "set" => {
                    let entity = get_str("entity").or_else(|| get_str("device_id"))?;
                    let state = get_str("state").or_else(|| get_str("value"))?;
                    Some(format!("--set {} {}", entity, state))
                }
                _ => None,
            }
        })(),
        "spotify" => (|| {
            let action = get_str("action")?;
            let query = get_str("query").or_else(|| get_str("track"));
            match action.as_str() {
                // No dedicated search subcommand — --play takes a free-text query.
                "play" | "search" | "next" | "previous" => {
                    query.map(|q| format!("--play {}", q))
                }
                "pause" => Some("--pause".to_string()),
                "resume" => Some("--resume".to_string()),
                "queue" => query.map(|q| format!("--queue {}", q)),
                "now-playing" | "now_playing" | "nowplaying" => Some("--now-playing".to_string()),
                _ => None,
            }
        })(),
        // Platform capabilities
        "yuanbao" => {
            if let Some(action) = get_str("action") {
                let mut result = action;
                if let Some(group) = get_str("group") {
                    result.push_str(&format!(" --group {}", group));
                }
                if let Some(message) = get_str("message") {
                    result.push_str(&format!(" {}", message));
                }
                Some(result)
            } else {
                None
            }
        }
        "computer_use" => {
            if let Some(action) = get_str("action") {
                let mut result = action;
                if let (Some(x), Some(y)) = (get_str("x"), get_str("y")) {
                    result.push_str(&format!(" {} {}", x, y));
                }
                if let Some(text) = get_str("text") {
                    result.push_str(&format!(" {}", text));
                }
                Some(result)
            } else {
                None
            }
        }
        // Agentic capabilities
        "moa" => get_str("task").map(|task| task),
        "delegation" => {
            if let (Some(agent), Some(task)) = (get_str("agent"), get_str("task")) {
                Some(format!("{} {}", agent, task))
            } else {
                None
            }
        }
        "clarify" => get_str("question").map(|question| question),
        // Execution capabilities
        "code_execution" => (|| {
            // Emit bare code only — the tool is python3-only and treats the whole
            // arg string as source (a "python <code>" prefix lands IN the file).
            let code = get_str("code")?;
            let mut result = code;
            if let Some(timeout) = get_str("timeout") {
                result.push_str(&format!(" --timeout {}", timeout));
            }
            if let Some(venv) = get_str("venv") {
                result.push_str(&format!(" --venv {}", venv));
            }
            Some(result)
        })(),
        "android" => (|| {
            // Schema exposes a single "command" string ('<category> <operation> <args>');
            // also tolerate split fields.
            if let Some(command) = get_str("command") {
                return Some(command);
            }
            let category = get_str("category")?;
            let mut result = category;
            if let Some(op) = get_str("operation").or_else(|| get_str("action")) {
                result.push_str(&format!(" {}", op));
            }
            if let Some(rest) = get_str("args").or_else(|| get_str("value")) {
                result.push_str(&format!(" {}", rest));
            }
            Some(result)
        })(),
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

/// Heuristic: does a tool's `Ok(...)` output actually report a failure?
/// Many tools return usage/error text as `Ok(String)` instead of `Err`,
/// which used to make every failed call record `success = true` in the DB.
/// Conservative: only matches on the *start* of the output to avoid
/// false positives from file contents that merely contain these phrases.
pub fn tool_output_indicates_failure(output: &str) -> bool {
    let head = output.trim_start();
    const FAILURE_PREFIXES: &[&str] = &[
        "Usage:",
        "Unknown ",
        "String not found",
        "Failed to",
        "Tool execution failed",
        "Security denied",
        "Approval required",
        "Not a git repository",
        "fatal:",
        "Error:",
        "error:",
        "No FAL_KEY",
        "No test framework detected",
        "No Python code provided",
        "No session database found",
        "No results found",
    ];
    FAILURE_PREFIXES.iter().any(|p| head.starts_with(p))
}

#[cfg(test)]
mod normalize_tests {
    use super::*;

    #[test]
    fn edit_json_operation_write() {
        let args = r#"{"operation":"write","file":"/tmp/demo.txt","new_string":"hello world"}"#;
        assert_eq!(
            normalize_tool_args("edit", args),
            "write /tmp/demo.txt hello world"
        );
    }

    #[test]
    fn edit_json_operation_write_with_content_key() {
        let args = r#"{"operation":"write","path":"/tmp/demo.txt","content":"line1\nline2"}"#;
        assert_eq!(
            normalize_tool_args("edit", args),
            "write /tmp/demo.txt line1\nline2"
        );
    }

    #[test]
    fn edit_json_replace_uses_delimiter() {
        let args = r#"{"file":"/tmp/demo.txt","old_string":"foo bar","new_string":"baz"}"#;
        assert_eq!(
            normalize_tool_args("edit", args),
            "replace /tmp/demo.txt foo bar ||| baz"
        );
    }

    #[test]
    fn edit_json_operation_read() {
        let args = r#"{"operation":"read","file":"/tmp/demo.txt"}"#;
        assert_eq!(normalize_tool_args("edit", args), "read /tmp/demo.txt");
    }

    #[test]
    fn edit_plain_args_pass_through() {
        let args = "replace /tmp/demo.txt foo ||| bar";
        assert_eq!(normalize_tool_args("edit", args), args);
    }

    #[test]
    fn failure_heuristic_catches_usage_errors() {
        assert!(tool_output_indicates_failure(
            "Unknown edit command: {\"operation\":\"write\"}"
        ));
        assert!(tool_output_indicates_failure("Usage: edit read <path>"));
        assert!(tool_output_indicates_failure(
            "String not found in /tmp/x. Use 'edit read' to see exact content."
        ));
        assert!(tool_output_indicates_failure(
            "Tool 'terminal' timed out after 120s"
        ) == false); // doesn't start with a known prefix — acceptable
    }

    #[test]
    fn failure_heuristic_passes_real_output() {
        assert!(!tool_output_indicates_failure("Written 57 bytes to /tmp/demo.txt"));
        assert!(!tool_output_indicates_failure("Replaced in /tmp/demo.txt"));
        assert!(!tool_output_indicates_failure("   1| fn main() {}"));
    }

    // ── 2026-07-27 protocol-test regressions ─────────────────────────────
    // Every case below is a real failure observed in memory.db when K3 ran
    // "test all of your toolcalls" — raw JSON leaking into CLI-style tools.

    #[test]
    fn web_query_does_not_leak_json() {
        let args = r#"{"query":"openshark protocol"}"#;
        assert_eq!(
            normalize_tool_args("web", args),
            "openshark protocol"
        );
    }

    #[test]
    fn web_scrape_maps_to_flag() {
        let args = r#"{"action":"scrape","url":"https://example.com"}"#;
        assert_eq!(
            normalize_tool_args("web", args),
            "--scrape https://example.com"
        );
    }

    #[test]
    fn browser_visit_maps_to_navigate() {
        let args = r#"{"action":"visit","url":"https://example.com"}"#;
        assert_eq!(
            normalize_tool_args("browser", args),
            "--navigate https://example.com"
        );
    }

    #[test]
    fn browser_url_only_implies_navigate() {
        let args = r#"{"url":"https://example.com"}"#;
        assert_eq!(
            normalize_tool_args("browser", args),
            "--navigate https://example.com"
        );
    }

    #[test]
    fn browser_type_needs_selector_and_text() {
        let args = r##"{"action":"type","selector":"#q","text":"hello"}"##;
        assert_eq!(normalize_tool_args("browser", args), "--type #q hello");
    }

    #[test]
    fn spotify_search_falls_back_to_play() {
        let args = r#"{"action":"search","query":"synthwave"}"#;
        assert_eq!(normalize_tool_args("spotify", args), "--play synthwave");
    }

    #[test]
    fn spotify_now_playing_variants() {
        assert_eq!(
            normalize_tool_args("spotify", r#"{"action":"now_playing"}"#),
            "--now-playing"
        );
    }

    #[test]
    fn memory_add_accepts_query_key() {
        let args = r#"{"action":"add","query":"the grid is endless"}"#;
        assert_eq!(
            normalize_tool_args("memory", args),
            "--add the grid is endless --target memory"
        );
    }

    #[test]
    fn memory_search_and_list() {
        assert_eq!(
            normalize_tool_args("memory", r#"{"action":"search","query":"neon"}"#),
            "--search neon"
        );
        assert_eq!(
            normalize_tool_args("memory", r#"{"action":"list"}"#),
            "--list"
        );
    }

    #[test]
    fn session_search_extracts_query_and_limit() {
        let args = r#"{"limit":3,"query":"protocol test tools"}"#;
        assert_eq!(
            normalize_tool_args("session_search", args),
            "protocol test tools --limit 3"
        );
    }

    #[test]
    fn skills_action_list() {
        assert_eq!(
            normalize_tool_args("skills", r#"{"action":"list"}"#),
            "--list"
        );
    }

    #[test]
    fn skills_view_uses_name() {
        assert_eq!(
            normalize_tool_args("skills", r#"{"action":"view","name":"rust"}"#),
            "--view rust"
        );
    }

    #[test]
    fn homeassistant_toggle_and_default_list() {
        assert_eq!(
            normalize_tool_args("homeassistant", r#"{"action":"toggle","entity":"light.living_room"}"#),
            "--toggle light.living_room"
        );
        assert_eq!(
            normalize_tool_args("homeassistant", r#"{"action":"list"}"#),
            "--list"
        );
        // args passthrough still works (observed working in the wild)
        assert_eq!(
            normalize_tool_args("homeassistant", r#"{"args":"--list"}"#),
            "--list"
        );
    }

    #[test]
    fn android_command_passes_through() {
        let args = r#"{"command":"device info"}"#;
        assert_eq!(normalize_tool_args("android", args), "device info");
    }

    #[test]
    fn android_split_fields() {
        let args = r#"{"category":"files","operation":"list","args":"/sdcard"}"#;
        assert_eq!(
            normalize_tool_args("android", args),
            "files list /sdcard"
        );
    }

    #[test]
    fn code_execution_without_language() {
        let args = r#"{"code":"print(2+2)"}"#;
        assert_eq!(normalize_tool_args("code_execution", args), "print(2+2)");
    }

    #[test]
    fn code_execution_strips_language_prefix() {
        // Language must NOT end up inside the temp .py file
        let args = r#"{"language":"python","code":"print(2+2)"}"#;
        assert_eq!(normalize_tool_args("code_execution", args), "print(2+2)");
    }
}
