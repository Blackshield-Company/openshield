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

pub fn get_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(edit::EditTool),
        Box::new(fs::FsTool),
        Box::new(git::GitTool),
        Box::new(lsp::LspTool),
        Box::new(refactor::RefactorTool),
        Box::new(search::SearchTool),
        Box::new(search::GrepTool),
        Box::new(terminal::TerminalTool),
        Box::new(test_runner::TestTool),
    ]
}

pub fn find_tool(name: &str) -> Option<Box<dyn Tool>> {
    get_tools()
        .into_iter()
        .find(|tool| tool.name() == name)
}
