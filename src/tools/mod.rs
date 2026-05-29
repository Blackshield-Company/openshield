pub mod edit;
pub mod fs;
pub mod git;
pub mod search;
pub mod terminal;

use anyhow::Result;

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
        Box::new(search::SearchTool),
        Box::new(search::GrepTool),
        Box::new(terminal::TerminalTool),
    ]
}

pub fn find_tool(name: &str) -> Option<Box<dyn Tool>> {
    let tools = get_tools();
    for tool in tools {
        if tool.name() == name {
            return Some(tool);
        }
    }
    None
}
