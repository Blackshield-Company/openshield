//! Guardian — Code Review Agent
//!
//! An autonomous code reviewer that analyzes changes for:
//! - Bugs and logic errors
//! - Security vulnerabilities
//! - Performance issues
//! - Style and best practice violations
//! - Test coverage gaps
//!
//! Usage: `/review [file]` or `/review` for recent changes

use anyhow::Result;
use std::path::Path;

/// A review finding with severity and location.
#[derive(Debug, Clone)]
pub struct ReviewFinding {
    pub severity: Severity,
    pub category: Category,
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Critical => write!(f, "🔴 Critical"),
            Severity::Warning => write!(f, "🟡 Warning"),
            Severity::Info => write!(f, "🔵 Info"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Bug,
    Security,
    Performance,
    Style,
    Maintainability,
    TestCoverage,
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Category::Bug => write!(f, "🐛 Bug"),
            Category::Security => write!(f, "🔒 Security"),
            Category::Performance => write!(f, "⚡ Performance"),
            Category::Style => write!(f, "🎨 Style"),
            Category::Maintainability => write!(f, "🔧 Maintainability"),
            Category::TestCoverage => write!(f, "🧪 Test Coverage"),
        }
    }
}

/// Run a guardian review on the given target.
/// Target can be a file path, "recent" for git diff, or "all" for full codebase.
pub async fn review(
    target: &str,
    project_path: &str,
    provider: crate::providers::Provider,
    model: String,
) -> Result<ReviewReport> {
    let mut findings: Vec<ReviewFinding> = Vec::new();

    // Gather context based on target
    let context = match target {
        "recent" => gather_recent_changes(project_path).await?,
        "all" => gather_codebase_overview(project_path).await?,
        file => gather_file_context(project_path, file).await?,
    };

    // Build review prompt
    let prompt = format!(
        "You are a senior code reviewer. Review the following code changes and identify issues.\n\n\
        For each issue found, respond in this exact format:\n\
        SEVERITY: [CRITICAL|WARNING|INFO]\n\
        CATEGORY: [BUG|SECURITY|PERFORMANCE|STYLE|MAINTAINABILITY|TEST_COVERAGE]\n\
        FILE: <filename>\n\
        LINE: <line number or N/A>\n\
        MESSAGE: <detailed description>\n\
        SUGGESTION: <specific fix or improvement>\n\
        ---\n\n\
        Code to review:\n\
        ```\n{}\n```\n\n\
        If no issues found, respond with: NO_ISSUES_FOUND",
        context
    );

    let request = crate::providers::ChatRequest {
        model,
        messages: vec![crate::providers::Message {
            role: "user".to_string(),
            content: prompt,
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }],
        stream: false,
        max_tokens: Some(4000),
        temperature: Some(0.2),
        tools: None,
    };

    let response = provider.chat(request).await?;
    let content = response
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default();

    let parsed_findings = parse_findings(&content);

    Ok(ReviewReport {
        target: target.to_string(),
        findings: parsed_findings.clone(),
        summary: if parsed_findings.is_empty() {
            "✅ No issues found — code looks good!".to_string()
        } else {
            format!(
                "Found {} issue(s): {} critical, {} warnings, {} info",
                parsed_findings.len(),
                parsed_findings.iter().filter(|f| f.severity == Severity::Critical).count(),
                parsed_findings.iter().filter(|f| f.severity == Severity::Warning).count(),
                parsed_findings.iter().filter(|f| f.severity == Severity::Info).count(),
            )
        },
    })
}

/// Parse structured findings from LLM response.
fn parse_findings(content: &str) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();

    if content.trim() == "NO_ISSUES_FOUND" {
        return findings;
    }

    for block in content.split("---") {
        let mut severity = None;
        let mut category = None;
        let mut file = String::new();
        let mut line = None;
        let mut message = String::new();
        let mut suggestion = None;

        for line_text in block.lines() {
            let trimmed = line_text.trim();
            if trimmed.starts_with("SEVERITY:") {
                severity = Some(match trimmed[9..].trim() {
                    "CRITICAL" => Severity::Critical,
                    "WARNING" => Severity::Warning,
                    _ => Severity::Info,
                });
            } else if trimmed.starts_with("CATEGORY:") {
                category = Some(match trimmed[9..].trim() {
                    "BUG" => Category::Bug,
                    "SECURITY" => Category::Security,
                    "PERFORMANCE" => Category::Performance,
                    "STYLE" => Category::Style,
                    "MAINTAINABILITY" => Category::Maintainability,
                    "TEST_COVERAGE" => Category::TestCoverage,
                    _ => Category::Maintainability,
                });
            } else if trimmed.starts_with("FILE:") {
                file = trimmed[5..].trim().to_string();
            } else if trimmed.starts_with("LINE:") {
                let line_str = trimmed[5..].trim();
                line = line_str.parse().ok();
            } else if trimmed.starts_with("MESSAGE:") {
                message = trimmed[8..].trim().to_string();
            } else if trimmed.starts_with("SUGGESTION:") {
                suggestion = Some(trimmed[11..].trim().to_string());
            }
        }

        if let (Some(sev), Some(cat)) = (severity, category) {
            if !message.is_empty() {
                findings.push(ReviewFinding {
                    severity: sev,
                    category: cat,
                    file,
                    line,
                    message,
                    suggestion,
                });
            }
        }
    }

    findings
}

/// Gather recent git changes for review.
async fn gather_recent_changes(project_path: &str) -> Result<String> {
    let output = tokio::process::Command::new("git")
        .args(["diff", "HEAD~1", "--stat"])
        .current_dir(project_path)
        .output()
        .await?;

    let stat = String::from_utf8_lossy(&output.stdout);

    let diff_output = tokio::process::Command::new("git")
        .args(["diff", "HEAD~1"])
        .current_dir(project_path)
        .output()
        .await?;

    let diff = String::from_utf8_lossy(&diff_output.stdout);

    Ok(format!("Changed files:\n{}\n\nDiff:\n{}", stat, diff))
}

/// Gather context for a specific file.
async fn gather_file_context(project_path: &str, file: &str) -> Result<String> {
    let path = Path::new(project_path).join(file);
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(format!("File: {}\n\n```\n{}\n```", file, content))
}

/// Gather codebase overview for full review.
async fn gather_codebase_overview(project_path: &str) -> Result<String> {
    let output = tokio::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(project_path)
        .output()
        .await?;

    let files = String::from_utf8_lossy(&output.stdout);
    let file_list: Vec<&str> = files.lines().collect();

    // Limit to first 50 source files to avoid overwhelming the LLM
    let limited: Vec<&str> = file_list.into_iter().filter(|f| {
        f.ends_with(".rs") || f.ends_with(".py") || f.ends_with(".js") || f.ends_with(".ts")
    }).take(50).collect();

    Ok(format!(
        "Codebase files ({} shown):\n{}\n\nRun `/review <file>` for detailed review of a specific file.",
        limited.len(),
        limited.join("\n")
    ))
}

/// Report from a guardian review.
#[derive(Debug, Clone)]
pub struct ReviewReport {
    pub target: String,
    pub findings: Vec<ReviewFinding>,
    pub summary: String,
}

impl ReviewReport {
    /// Format the report as a readable message.
    pub fn format(&self) -> String {
        let mut lines = vec![
            format!("🛡️  Guardian Review: {}", self.target),
            format!("{}", "═".repeat(50)),
            self.summary.clone(),
            String::new(),
        ];

        for finding in &self.findings {
            lines.push(format!(
                "{} | {} | {}",
                finding.severity, finding.category, finding.file
            ));
            if let Some(line) = finding.line {
                lines.push(format!("   Line: {}", line));
            }
            lines.push(format!("   {}", finding.message));
            if let Some(suggestion) = &finding.suggestion {
                lines.push(format!("   💡 Suggestion: {}", suggestion));
            }
            lines.push(String::new());
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_findings() {
        let content = r#"
SEVERITY: CRITICAL
CATEGORY: SECURITY
FILE: src/auth.rs
LINE: 42
MESSAGE: SQL injection vulnerability in user input handling
SUGGESTION: Use parameterized queries
---
SEVERITY: WARNING
CATEGORY: STYLE
FILE: src/main.rs
LINE: 10
MESSAGE: Unused import
SUGGESTION: Remove the import
"#;
        let findings = parse_findings(content);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].category, Category::Security);
        assert_eq!(findings[1].severity, Severity::Warning);
    }

    #[test]
    fn test_parse_no_issues() {
        let findings = parse_findings("NO_ISSUES_FOUND");
        assert!(findings.is_empty());
    }

    #[test]
    fn test_report_format() {
        let report = ReviewReport {
            target: "src/main.rs".to_string(),
            findings: vec![ReviewFinding {
                severity: Severity::Warning,
                category: Category::Style,
                file: "src/main.rs".to_string(),
                line: Some(5),
                message: "Long function".to_string(),
                suggestion: Some("Split into smaller functions".to_string()),
            }],
            summary: "Found 1 issue".to_string(),
        };
        let formatted = report.format();
        assert!(formatted.contains("Guardian Review"));
        assert!(formatted.contains("Long function"));
    }
}
