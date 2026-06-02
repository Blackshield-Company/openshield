use super::Tool;
use anyhow::{Context, Result};
use std::process::Command;

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
            result.push_str(&format!(
                "\n[stderr]: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_echo() {
        let tool = TerminalTool;
        let result = tool.execute("echo 'Hello World'").unwrap();
        assert!(result.contains("Hello World"));
    }

    #[test]
    fn test_terminal_pwd() {
        let tool = TerminalTool;
        let result = tool.execute("pwd").unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_terminal_invalid_command() {
        let tool = TerminalTool;
        let result = tool.execute("this_command_should_not_exist_12345").unwrap();
        assert!(result.contains("[stderr]") || result.is_empty());
    }

    #[test]
    fn test_terminal_empty_command() {
        let tool = TerminalTool;
        let result = tool.execute("").unwrap();
        assert!(result.is_empty() || result.contains("[stderr]"));
    }
}
