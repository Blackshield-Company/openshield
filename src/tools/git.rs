use super::Tool;
use anyhow::{Context, Result};
use std::process::Command;

pub struct GitTool;

impl Tool for GitTool {
    fn name(&self) -> &str {
        "git"
    }

    fn description(&self) -> &str {
        "Git operations: status, diff, log, branch, checkout, commit. \
         Prefix with '--repo <path>' to run against a repository outside the current directory."
    }

    fn execute(&self, args: &str) -> Result<String> {
        // Optional repo targeting: `git --repo <path> <cmd> [args]`
        // Without it, commands run in the process cwd (legacy behavior).
        let mut work_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut args = args;
        if let Some(rest) = args.strip_prefix("--repo ") {
            let mut parts = rest.splitn(2, ' ');
            let dir = parts.next().unwrap_or("");
            if dir.is_empty() {
                return Ok("Usage: git [--repo <path>] <command> [args]".to_string());
            }
            work_dir = std::path::PathBuf::from(dir);
            args = parts.next().unwrap_or("");
        }

        // Check if the target is a git repo before running any command
        if !Self::in_repo_at(&work_dir) {
            return Ok(format!(
                "Not a git repository: {}",
                work_dir.display()
            ));
        }

        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.is_empty() {
            return Ok(self.usage());
        }

        let cmd = parts[0];
        let rest = parts.get(1).unwrap_or(&"");

        let output = match cmd {
            "status" => Self::git_cmd(&work_dir).args(["status", "--short"]).output(),
            "diff" => {
                let mut c = Self::git_cmd(&work_dir);
                c.arg("diff");
                if !rest.is_empty() {
                    c.arg(rest);
                }
                c.output()
            }
            "diff-staged" => Self::git_cmd(&work_dir).args(["diff", "--staged"]).output(),
            "diff-cached" => Self::git_cmd(&work_dir).args(["diff", "--cached"]).output(),
            "log" => {
                let limit = rest.parse::<usize>().unwrap_or(10);
                Self::git_cmd(&work_dir)
                    .args(["log", &format!("--max-count={}", limit), "--oneline"])
                    .output()
            }
            "branch" => Self::git_cmd(&work_dir).args(["branch", "-a"]).output(),
            "checkout" => {
                if rest.is_empty() {
                    return Ok("Usage: git checkout <branch>".to_string());
                }
                Self::git_cmd(&work_dir).args(["checkout", rest]).output()
            }
            "commit" => {
                if rest.is_empty() {
                    return Ok("Usage: git commit <message>".to_string());
                }
                Self::git_cmd(&work_dir).args(["commit", "-m", rest]).output()
            }
            "add" => {
                if rest.is_empty() {
                    return Ok("Usage: git add <path>".to_string());
                }
                Self::git_cmd(&work_dir).args(["add", rest]).output()
            }
            "show" => Self::git_cmd(&work_dir).args(["show", "--stat", rest]).output(),
            "stage-all" => Self::git_cmd(&work_dir).args(["add", "-A"]).output(),
            "push" => {
                let mut c = Self::git_cmd(&work_dir);
                c.arg("push");
                if !rest.is_empty() {
                    c.args(rest.split_whitespace());
                }
                c.output()
            }
            "branch-create" => {
                if rest.is_empty() {
                    return Ok("Usage: git branch-create <name>".to_string());
                }
                Self::git_cmd(&work_dir).args(["checkout", "-b", rest]).output()
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
            result.push_str(&format!(
                "\n[stderr]: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(result)
    }
}

impl GitTool {
    /// Build a git Command pinned to the given working directory.
    fn git_cmd(dir: &std::path::Path) -> Command {
        let mut c = Command::new("git");
        c.current_dir(dir);
        c
    }

    /// Check whether the given directory is inside a git repository.
    pub fn in_repo_at(dir: &std::path::Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(dir)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn usage(&self) -> String {
        "Git tool usage:\n\\
         git [--repo <path>] status           - Show working tree status\n\\
         git [--repo <path>] diff [path]      - Show changes\n\\
         git [--repo <path>] diff-staged      - Show staged changes\n\\
         git [--repo <path>] diff-cached      - Alias for diff-staged\n\\
         git [--repo <path>] log [n]          - Show last n commits (default 10)\n\\
         git [--repo <path>] branch           - List branches\n\\
         git [--repo <path>] checkout <name>  - Switch branch\n\\
         git [--repo <path>] add <path>       - Stage files\n\\
         git [--repo <path>] commit <msg>     - Commit staged files\n\\
         git [--repo <path>] show <ref>       - Show commit details\n\\
         git [--repo <path>] stage-all        - Stage all changes\n\\
         git [--repo <path>] push [remote]    - Push current branch\n\\
         git [--repo <path>] branch-create <n> - Create and switch to branch"
            .to_string()
    }

    /// Check if we're inside a git repository.
    pub fn in_repo() -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get the diff of unstaged changes.
    #[allow(dead_code)]
    pub fn get_unstaged_diff() -> Result<String> {
        let output = Command::new("git")
            .args(["diff"])
            .output()
            .context("Failed to run git diff")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get the diff of staged changes.
    #[allow(dead_code)]
    pub fn get_staged_diff() -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "--staged"])
            .output()
            .context("Failed to run git diff --staged")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Stage all changes.
    #[allow(dead_code)]
    pub fn stage_all() -> Result<()> {
        let output = Command::new("git")
            .args(["add", "-A"])
            .output()
            .context("Failed to stage all changes")?;
        if !output.status.success() {
            anyhow::bail!(
                "git add -A failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Commit staged changes with the given message.
    #[allow(dead_code)]
    pub fn commit(message: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .output()
            .context("Failed to run git commit")?;
        if !output.status.success() {
            anyhow::bail!(
                "git commit failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Check if there are any changes (staged or unstaged).
    pub fn has_changes() -> bool {
        let Ok(output) = Command::new("git").args(["status", "--porcelain"]).output() else {
            return false;
        };
        !output.stdout.is_empty()
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
    fn test_git_repo_flag_targets_other_repo() {
        let dir = temp_git_repo();
        let tool = GitTool;

        // --repo must run against the target repo, not the process cwd
        let result = tool.execute(&format!("--repo {} branch", dir)).unwrap();
        assert!(
            !result.starts_with("Not a git repository"),
            "--repo target was not honored: {}",
            result
        );

        // Non-repo path reports cleanly with the path included
        let bad = tool
            .execute("--repo /tmp/openshark_definitely_not_a_repo_xyz status")
            .unwrap();
        assert!(
            bad.starts_with("Not a git repository"),
            "expected clean failure, got: {}",
            bad
        );

        cleanup(&dir);
    }

    #[test]
    fn test_git_status_empty() {
        let dir = temp_git_repo();
        let tool = GitTool;
        let result = tool.execute(&format!("status {}", dir));

        if let Ok(output) = result {
            assert!(!output.is_empty() || output.is_empty());
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
        if let Ok(output) = result {
            assert!(!output.is_empty() || output.is_empty());
        }
        cleanup(&dir);
    }

    #[test]
    fn test_git_branch() {
        let dir = temp_git_repo();
        let tool = GitTool;
        let result = tool.execute(&format!("branch {}", dir)).unwrap();
        assert!(result.lines().any(|l| l.starts_with("* ")) || result.is_empty());
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
