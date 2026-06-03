//! Linting Integration — Auto-run linters after edits
//!
//! Detects and runs the appropriate linter for the project type,
//! Linting Integration — Run project linters and surface results

#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "ERROR"),
            Severity::Warning => write!(f, "WARN"),
            Severity::Info => write!(f, "INFO"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintResult {
    pub tool: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub severity: Severity,
    pub code: Option<String>,
}

/// Detect which linter to use based on project files.
pub fn detect_linter(root: &str) -> Option<String> {
    let root = Path::new(root);

    if root.join("Cargo.toml").exists() {
        return Some("cargo clippy".to_string());
    }
    if root.join("package.json").exists() {
        if root.join("eslint.config.js").exists()
            || root.join(".eslintrc").exists()
            || root.join(".eslintrc.json").exists()
        {
            return Some("eslint".to_string());
        }
        return Some("npm run lint".to_string());
    }
    if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        return Some("ruff check".to_string());
    }
    if root.join("go.mod").exists() {
        return Some("gofmt -l".to_string());
    }
    if root.join("Makefile").exists() {
        return Some("make lint".to_string());
    }

    None
}

/// Run the detected linter and parse results.
pub async fn run_linter(path: &str) -> Result<Vec<LintResult>> {
    let linter = detect_linter(path)
        .ok_or_else(|| anyhow::anyhow!("No linter detected for project at {}", path))?;

    match linter.as_str() {
        "cargo clippy" => run_cargo_clippy(path).await,
        "eslint" => run_eslint(path).await,
        "npm run lint" => run_npm_lint(path).await,
        "ruff check" => run_ruff(path).await,
        "gofmt -l" => run_gofmt(path).await,
        "make lint" => run_make_lint(path).await,
        _ => Ok(vec![]),
    }
}

async fn run_cargo_clippy(path: &str) -> Result<Vec<LintResult>> {
    let output = Command::new("cargo")
        .args(["clippy", "--message-format=short", "--", "-Dwarnings"])
        .current_dir(path)
        .output()
        .context("Failed to run cargo clippy")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut results = Vec::new();

    // Parse short format: "file.rs:line:col: severity: message"
    for line in stderr.lines() {
        if let Some(result) = parse_short_format(line, "clippy") {
            results.push(result);
        }
    }

    Ok(results)
}

async fn run_eslint(path: &str) -> Result<Vec<LintResult>> {
    let output = Command::new("npx")
        .args(["eslint", "--format", "compact", "."])
        .current_dir(path)
        .output()
        .context("Failed to run eslint")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    // Compact format: "file.js: line 1, col 2, Error - message"
    for line in stdout.lines() {
        if let Some(result) = parse_eslint_compact(line) {
            results.push(result);
        }
    }

    Ok(results)
}

async fn run_npm_lint(path: &str) -> Result<Vec<LintResult>> {
    let output = Command::new("npm")
        .args(["run", "lint"])
        .current_dir(path)
        .output()
        .context("Failed to run npm lint")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let _stderr = String::from_utf8_lossy(&output.stderr);

    // Generic parsing — just capture the output as a single info result
    Ok(vec![LintResult {
        tool: "npm lint".to_string(),
        file: "-".to_string(),
        line: 0,
        column: 0,
        message: format!("{}", stdout),
        severity: Severity::Info,
        code: None,
    }])
}

async fn run_ruff(path: &str) -> Result<Vec<LintResult>> {
    let output = Command::new("ruff")
        .args(["check", "--output-format", "concise", "."])
        .current_dir(path)
        .output()
        .context("Failed to run ruff")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    // Concise format: "file.py:line:col: CODE message"
    for line in stdout.lines() {
        if let Some(result) = parse_ruff_output(line) {
            results.push(result);
        }
    }

    Ok(results)
}

async fn run_gofmt(path: &str) -> Result<Vec<LintResult>> {
    let output = Command::new("gofmt")
        .args(["-l", "."])
        .current_dir(path)
        .output()
        .context("Failed to run gofmt")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for file in stdout.lines() {
        if !file.trim().is_empty() {
            results.push(LintResult {
                tool: "gofmt".to_string(),
                file: file.to_string(),
                line: 0,
                column: 0,
                message: "File needs formatting".to_string(),
                severity: Severity::Warning,
                code: None,
            });
        }
    }

    Ok(results)
}

async fn run_make_lint(path: &str) -> Result<Vec<LintResult>> {
    let output = Command::new("make")
        .args(["lint"])
        .current_dir(path)
        .output()
        .context("Failed to run make lint")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(vec![LintResult {
        tool: "make lint".to_string(),
        file: "-".to_string(),
        line: 0,
        column: 0,
        message: stdout.to_string(),
        severity: Severity::Info,
        code: None,
    }])
}

fn parse_short_format(line: &str, tool: &str) -> Option<LintResult> {
    // "src/main.rs:42:5: warning: unused variable"
    let parts: Vec<&str> = line.splitn(2, ": ").collect();
    if parts.len() < 2 {
        return None;
    }

    let location = parts[0];
    let message = parts[1];

    let loc_parts: Vec<&str> = location.split(':').collect();
    if loc_parts.len() < 3 {
        return None;
    }

    let file = loc_parts[0].to_string();
    let line_num = loc_parts[1].parse().unwrap_or(0);
    let col = loc_parts[2].parse().unwrap_or(0);

    let severity = if message.contains("error") {
        Severity::Error
    } else if message.contains("warning") {
        Severity::Warning
    } else {
        Severity::Info
    };

    Some(LintResult {
        tool: tool.to_string(),
        file,
        line: line_num,
        column: col,
        message: message.to_string(),
        severity,
        code: None,
    })
}

fn parse_eslint_compact(line: &str) -> Option<LintResult> {
    // "file.js: line 1, col 2, Error - message"
    let re = regex::Regex::new(r"^(.*?):\s*line\s*(\d+),\s*col\s*(\d+),\s*(Error|Warning|Info)\s*-\s*(.*)$").ok()?;
    let caps = re.captures(line)?;

    Some(LintResult {
        tool: "eslint".to_string(),
        file: caps.get(1)?.as_str().to_string(),
        line: caps.get(2)?.as_str().parse().unwrap_or(0),
        column: caps.get(3)?.as_str().parse().unwrap_or(0),
        severity: match caps.get(4)?.as_str() {
            "Error" => Severity::Error,
            "Warning" => Severity::Warning,
            _ => Severity::Info,
        },
        message: caps.get(5)?.as_str().to_string(),
        code: None,
    })
}

fn parse_ruff_output(line: &str) -> Option<LintResult> {
    // "file.py:42:5: E501 Line too long"
    let parts: Vec<&str> = line.splitn(2, ": ").collect();
    if parts.len() < 2 {
        return None;
    }

    let location = parts[0];
    let rest = parts[1];

    let loc_parts: Vec<&str> = location.split(':').collect();
    if loc_parts.len() < 3 {
        return None;
    }

    let file = loc_parts[0].to_string();
    let line_num = loc_parts[1].parse().unwrap_or(0);
    let col = loc_parts[2].parse().unwrap_or(0);

    let code_msg: Vec<&str> = rest.splitn(2, ' ').collect();
    let code = code_msg.first().map(|s| s.to_string());
    let message = code_msg.get(1).unwrap_or(&"").to_string();

    Some(LintResult {
        tool: "ruff".to_string(),
        file,
        line: line_num,
        column: col,
        message,
        severity: Severity::Warning,
        code,
    })
}

pub fn format_lint_results(results: &[LintResult]) -> String {
    if results.is_empty() {
        return "✅ No lint issues found.".to_string();
    }

    let mut lines = vec![format!("🧹 Lint Results ({} issues)", results.len())];
    lines.push("─".repeat(50));

    for r in results {
        let icon = match r.severity {
            Severity::Error => "❌",
            Severity::Warning => "⚠️",
            Severity::Info => "ℹ️",
        };
        lines.push(format!(
            "{} {}:{} {} — {}",
            icon, r.file, r.line, r.severity, r.message
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_linter_rust() {
        let dir = format!("/tmp/openshark_lint_test_{}", std::process::id());
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{}/Cargo.toml", dir), "[package]\nname=\"test\"\n").unwrap();
        assert_eq!(detect_linter(&dir), Some("cargo clippy".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_short_format() {
        let line = "src/main.rs:42:5: warning: unused variable";
        let result = parse_short_format(line, "clippy").unwrap();
        assert_eq!(result.file, "src/main.rs");
        assert_eq!(result.line, 42);
        assert_eq!(result.severity, Severity::Warning);
    }

    #[test]
    fn test_format_lint_results_empty() {
        assert_eq!(format_lint_results(&[]), "✅ No lint issues found.");
    }
}
