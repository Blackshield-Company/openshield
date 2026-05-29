use anyhow::{Context, Result};
use regex::RegexBuilder;
use std::process::Command;
use super::Tool;

pub struct SearchTool;

impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search codebase with ripgrep. Usage: search <pattern> [path] [--ext rust]"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.is_empty() {
            return Ok("Usage: search <pattern> [path] [--ext <ext>]".to_string());
        }

        // First arg is the pattern
        let pattern = parts[0];
        let mut path = ".";
        let mut ext: Option<&str> = None;
        let mut ignore_case = false;

        let mut i = 1;
        while i < parts.len() {
            match parts[i] {
                "--ext" | "-e" => {
                    i += 1;
                    if i < parts.len() {
                        ext = Some(parts[i]);
                    }
                }
                "--ignore-case" | "-i" => {
                    ignore_case = true;
                }
                _ => {
                    // If it doesn't start with --, treat as path
                    if !parts[i].starts_with('-') {
                        path = parts[i];
                    }
                }
            }
            i += 1;
        }

        let mut cmd = Command::new("rg");
        cmd.arg("--line-number")
            .arg("--with-filename")
            .arg("--color=never")
            .arg("--max-count=50");

        if ignore_case {
            cmd.arg("--ignore-case");
        }

        if let Some(e) = ext {
            cmd.arg("--type").arg(e);
        }

        cmd.arg(pattern).arg(path);

        let output = cmd.output()
            .with_context(|| "Failed to run ripgrep. Is it installed?")?;

        let mut result = String::new();
        if !output.stdout.is_empty() {
            result.push_str(&String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("No files were searched") {
                result.push_str(&format!("\n[stderr]: {}", stderr));
            }
        }

        if result.trim().is_empty() {
            result = format!("No matches found for '{}' in {}", pattern, path);
        }

        Ok(result)
    }
}

pub struct GrepTool;

impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Regex search in file contents (fallback when rg unavailable)"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return Ok("Usage: grep <pattern> <path>".to_string());
        }

        let pattern = parts[0];
        let path = parts[1];

        let regex = RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
            .with_context(|| format!("Invalid regex: {}", pattern))?;

        let mut results = Vec::new();
        for entry in walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            if let Ok(content) = std::fs::read_to_string(path) {
                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        results.push(format!(
                            "{}:{}:{}",
                            path.display(),
                            line_num + 1,
                            line.trim()
                        ));
                        if results.len() >= 100 {
                            break;
                        }
                    }
                }
            }
            if results.len() >= 100 {
                break;
            }
        }

        if results.is_empty() {
            Ok(format!("No matches found for '{}' in {}", pattern, path))
        } else {
            Ok(results.join("\n"))
        }
    }
}
