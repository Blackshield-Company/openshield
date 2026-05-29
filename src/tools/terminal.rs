use anyhow::{Context, Result};
use std::process::Command;
use super::Tool;

pub struct TerminalTool;

impl Tool for TerminalTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn description(&self) -> &str {
        "Execute shell commands"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(args)
            .output()
            .with_context(|| format!("Failed to execute: {}", args))?;

        let mut result = String::new();
        if !output.stdout.is_empty() {
            result.push_str(&String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            result.push_str(&format!("\n[stderr]: {}", String::from_utf8_lossy(&output.stderr)));
        }

        Ok(result)
    }
}
