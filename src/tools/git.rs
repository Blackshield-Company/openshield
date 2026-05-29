use anyhow::{Context, Result};
use std::process::Command;
use super::Tool;

pub struct GitTool;

impl Tool for GitTool {
    fn name(&self) -> &str {
        "git"
    }

    fn description(&self) -> &str {
        "Git operations: status, diff, log, branch, checkout, commit"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.is_empty() {
            return Ok(self.usage());
        }

        let cmd = parts[0];
        let rest = parts.get(1).unwrap_or(&"");

        let output = match cmd {
            "status" => {
                Command::new("git")
                    .args(["status", "--short"])
                    .output()
            }
            "diff" => {
                let mut c = Command::new("git");
                c.arg("diff");
                if !rest.is_empty() {
                    c.arg(rest);
                }
                c.output()
            }
            "diff-staged" => {
                Command::new("git")
                    .args(["diff", "--staged"])
                    .output()
            }
            "log" => {
                let limit = rest.parse::<usize>().unwrap_or(10);
                Command::new("git")
                    .args(["log", &format!("--max-count={}", limit), "--oneline"])
                    .output()
            }
            "branch" => {
                Command::new("git")
                    .args(["branch", "-a"])
                    .output()
            }
            "checkout" => {
                if rest.is_empty() {
                    return Ok("Usage: git checkout <branch>".to_string());
                }
                Command::new("git")
                    .args(["checkout", rest])
                    .output()
            }
            "commit" => {
                if rest.is_empty() {
                    return Ok("Usage: git commit <message>".to_string());
                }
                Command::new("git")
                    .args(["commit", "-m", rest])
                    .output()
            }
            "add" => {
                if rest.is_empty() {
                    return Ok("Usage: git add <path>".to_string());
                }
                Command::new("git")
                    .args(["add", rest])
                    .output()
            }
            "show" => {
                Command::new("git")
                    .args(["show", "--stat", rest])
                    .output()
            }
            _ => {
                return Ok(format!("Unknown git command: {}\n{}", cmd, self.usage()));
            }
        };

        let output = output.with_context(|| format!("Failed to run git {}", cmd))?;

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

impl GitTool {
    fn usage(&self) -> String {
        "Git tool usage:\n\
         git status           - Show working tree status\n\
         git diff [path]      - Show changes\n\
         git diff-staged      - Show staged changes\n\
         git log [n]          - Show last n commits (default 10)\n\
         git branch           - List branches\n\
         git checkout <name>  - Switch branch\n\
         git add <path>       - Stage files\n\
         git commit <msg>     - Commit staged files\n\
         git show <ref>       - Show commit details".to_string()
    }
}
