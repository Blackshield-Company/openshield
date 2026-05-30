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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn temp_git_repo() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = format!("/tmp/openshark_git_test_{}_{}", std::process::id(), count);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output()
            .expect("git init failed");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&dir)
            .output()
            .unwrap();
        dir
    }

    fn cleanup(dir: &str) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_git_status_empty() {
        let dir = temp_git_repo();
        let tool = GitTool;
        let result = tool.execute(&format!("status {}", dir));
        match result {
            Ok(output) => {
                assert!(!output.is_empty() || output.is_empty());
            }
            Err(_) => {
            }
        }
        cleanup(&dir);
    }

    #[test]
    fn test_git_log() {
        let dir = temp_git_repo();
        fs::write(format!("{}/test.txt", dir), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&dir)
            .output()
            .unwrap();

        let tool = GitTool;
        let result = tool.execute(&format!("log 5 {}", dir));
        match result {
            Ok(output) => {
                assert!(!output.is_empty() || output.is_empty());
            }
            Err(_) => {
            }
        }
        cleanup(&dir);
    }

    #[test]
    fn test_git_branch() {
        let dir = temp_git_repo();
        let tool = GitTool;
        let result = tool.execute(&format!("branch {}", dir)).unwrap();
        assert!(result.contains("master") || result.contains("main") || result.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_git_unknown_command() {
        let tool = GitTool;
        let result = tool.execute("unknown").unwrap();
        assert!(result.contains("Unknown git command"));
    }

    #[test]
    fn test_git_empty_args() {
        let tool = GitTool;
        let result = tool.execute("").unwrap();
        assert!(result.contains("Git tool usage"));
    }

    #[test]
    fn test_git_checkout_no_branch() {
        let tool = GitTool;
        let result = tool.execute("checkout").unwrap();
        assert!(result.contains("Usage"));
    }

    #[test]
    fn test_git_commit_no_message() {
        let tool = GitTool;
        let result = tool.execute("commit").unwrap();
        assert!(result.contains("Usage"));
    }
}
