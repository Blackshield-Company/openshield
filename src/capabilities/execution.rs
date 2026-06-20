//! Execution capabilities — Python code execution.

use anyhow::{Context, Result};
use std::process::Stdio;

use crate::tools::Tool;

// ─── Code Execution Tool ────────────────────────────────────────────────────

pub struct CodeExecutionTool;

impl Tool for CodeExecutionTool {
    fn name(&self) -> &str {
        "code_execution"
    }
    fn description(&self) -> &str {
        "Execute Python code. Args: <python_code> [--timeout <secs>] [--venv <path>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return Ok(
                "Usage: code_execution <python_code> [--timeout <secs>] [--venv <path>]"
                    .to_string(),
            );
        }

        // Extract timeout
        let mut code = trimmed;
        let mut _timeout_secs = 30;
        let mut _venv = None;

        if let Some(pos) = code.rfind("--timeout") {
            let before = &code[..pos];
            let after = &code[pos + 9..];
            if let Some(secs) = after.split_whitespace().next()
                && let Ok(s) = secs.parse::<u64>()
            {
                _timeout_secs = s;
            }
            code = before.trim();
        }

        if let Some(pos) = code.rfind("--venv") {
            let before = &code[..pos];
            let after = &code[pos + 6..];
            _venv = after.split_whitespace().next().map(|s| s.to_string());
            code = before.trim();
        }

        // Strip markdown code block wrappers if present
        // Handles ```python, ```, ~~~python, etc.
        let code = if code.starts_with("```") || code.starts_with("~~~") {
            let mut lines: Vec<&str> = code.lines().collect();
            // Remove first line if it's a code fence with optional language
            if !lines.is_empty() && (lines[0].starts_with("```") || lines[0].starts_with("~~~")) {
                lines.remove(0);
            }
            // Remove last line if it's a code fence
            if !lines.is_empty() && (lines[lines.len() - 1].trim() == "```" || lines[lines.len() - 1].trim() == "~~~") {
                lines.pop();
            }
            lines.join("\n")
        } else if code.starts_with("python") || code.starts_with("Python") {
            // Strip leading "python" or "Python" language identifier
            code.strip_prefix("python").unwrap_or(code).strip_prefix("Python").unwrap_or(code).trim().to_string()
        } else {
            code.to_string()
        };

        if code.is_empty() {
            return Ok("No Python code provided.".to_string());
        }

        // Write code to temp file and execute
        let tmp_dir = std::env::temp_dir();
        let tmp_file = tmp_dir.join(format!("openshark_exec_{}.py", uuid::Uuid::new_v4()));

        std::fs::write(&tmp_file, &code)
            .with_context(|| format!("Failed to write temp file: {:?}", tmp_file))?;

        let output = std::process::Command::new("python3")
            .arg(&tmp_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| "Failed to execute Python code")?;

        // Clean up temp file
        let _ = std::fs::remove_file(&tmp_file);

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str("Output:\n");
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("Stderr:\n");
            result.push_str(&stderr);
        }
        if result.is_empty() {
            result = "Code executed successfully (no output).".to_string();
        }

        if !output.status.success() {
            result.push_str(&format!("\nExit code: {:?}", output.status.code()));
        }

        Ok(result)
    }
}
