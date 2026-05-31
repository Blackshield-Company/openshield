pub mod r#async;
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
pub use detection::{detect_tool_suggestions, ToolSuggestion};

use anyhow::Result;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Tool definition for schema-based tools (e.g., MCP tools).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn execute(&self, args: &str) -> Result<String>;
}

/// Global cache for MCP-discovered tools, populated after MCP initialization.
static MCP_TOOLS: Mutex<Vec<Arc<dyn Tool>>> = Mutex::new(Vec::new());

/// Register MCP tools into the global cache. Called after MCP discovery.
pub fn register_mcp_tools(tools: Vec<Arc<dyn Tool>>) {
    if let Ok(mut guard) = MCP_TOOLS.lock() {
        guard.clear();
        guard.extend(tools);
    }
}

/// Get all native + MCP tools.
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
    if let Ok(mcp) = MCP_TOOLS.lock() {
        for tool in mcp.iter() {
            tools.push(Arc::clone(tool));
        }
    }
    tools
}

pub fn find_tool(name: &str) -> Option<Arc<dyn Tool>> {
    get_tools()
        .into_iter()
        .find(|tool| tool.name() == name)
}
