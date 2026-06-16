//! Checkpoints / Undo system for OpenShark
//!
//! Git-based checkpoints that save the working tree state before agent edits.
//! Users can manually save checkpoints with `/checkpoint`, undo with `/undo`,
//! and redo with `/redo`.
//!
//! Implementation uses git stash for lightweight snapshots and temp branches
//! for named checkpoints that need to persist across undo/redo.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Metadata for a single checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub name: String,
    pub created_at: DateTime<Utc>,
    /// Git stash ref (e.g. "stash@{0}") or temp branch name.
    pub git_ref: String,
    /// Whether this is a stash (true) or temp branch (false).
    pub is_stash: bool,
    /// Optional description.
    pub description: Option<String>,
}

impl Checkpoint {
    pub fn new(name: impl Into<String>, git_ref: impl Into<String>, is_stash: bool) -> Self {
        Self {
            name: name.into(),
            created_at: Utc::now(),
            git_ref: git_ref.into(),
            is_stash,
            description: None,
        }
    }
}

/// Per-session checkpoint stack with undo/redo support.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckpointStack {
    /// Checkpoints that can be undone (newest at end).
    pub undo_stack: Vec<Checkpoint>,
    /// Checkpoints that can be redone (newest at end).
    pub redo_stack: Vec<Checkpoint>,
    /// Session ID this stack belongs to.
    pub session_id: String,
}

impl CheckpointStack {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            session_id: session_id.into(),
        }
    }

    /// Push a new checkpoint onto the undo stack, clearing redo.
    pub fn push(&mut self, checkpoint: Checkpoint) {
        self.undo_stack.push(checkpoint);
        self.redo_stack.clear();
    }

    /// Pop the latest checkpoint for undo.
    #[allow(dead_code)]
    pub fn pop_undo(&mut self) -> Option<Checkpoint> {
        self.undo_stack.pop()
    }

    /// Pop the latest checkpoint for redo.
    #[allow(dead_code)]
    pub fn pop_redo(&mut self) -> Option<Checkpoint> {
        self.redo_stack.pop()
    }

    /// Length of undo stack.
    #[allow(dead_code)]
    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    /// Length of redo stack.
    #[allow(dead_code)]
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }

    /// Peek at the top undo checkpoint without removing.
    #[allow(dead_code)]
    pub fn peek_undo(&self) -> Option<&Checkpoint> {
        self.undo_stack.last()
    }

    /// Undo: pop from undo, restore via git, push to redo.
    pub fn undo(&mut self) -> Result<String> {
        let cp = self
            .undo_stack
            .pop()
            .ok_or_else(|| anyhow::anyhow!("No checkpoints to undo"))?;
        restore_checkpoint(&cp)?;
        self.redo_stack.push(cp.clone());
        Ok(cp.name)
    }

    /// Redo: pop from redo, restore via git, push back to undo.
    pub fn redo(&mut self) -> Result<String> {
        let cp = self
            .redo_stack
            .pop()
            .ok_or_else(|| anyhow::anyhow!("No checkpoints to redo"))?;
        restore_checkpoint(&cp)?;
        self.undo_stack.push(cp.clone());
        Ok(cp.name)
    }

    /// Save a new checkpoint: create via git, push to undo, clear redo.
    pub fn save(&mut self, name: &str) -> Result<()> {
        let cp = save_checkpoint(name)?;
        self.undo_stack.push(cp);
        self.redo_stack.clear();
        Ok(())
    }

    /// Push a checkpoint onto the redo stack.
    pub fn push_redo(&mut self, checkpoint: Checkpoint) {
        self.redo_stack.push(checkpoint);
    }
}

/// Create a git stash checkpoint.
/// Returns the stash ref (e.g. "stash@{0}") on success.
pub fn stash_checkpoint(name: &str) -> Result<String> {
    let msg = format!("openshark-checkpoint: {}", name);
    let output = Command::new("git")
        .args(["stash", "push", "-m", &msg, "--include-untracked"])
        .output()
        .context("Failed to run git stash push")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Git returns success (0) but prints "No local changes to save" to stdout when clean.
    if stdout.contains("No local changes") || stdout.contains("nothing to stash") {
        return Ok("clean".to_string());
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git stash failed: {}", stderr);
    }

    // Get the stash ref we just created
    let list = Command::new("git")
        .args(["stash", "list"])
        .output()
        .context("Failed to list stashes")?;

    let list_str = String::from_utf8_lossy(&list.stdout);
    // Find the first stash with our message
    for line in list_str.lines() {
        if line.contains(&msg)
            && let Some(ref_part) = line.split(':').next()
        {
            return Ok(ref_part.trim().to_string());
        }
    }

    // Fallback
    Ok("stash@{0}".to_string())
}

/// Restore from a stash ref.
pub fn restore_stash(stash_ref: &str) -> Result<String> {
    if stash_ref == "clean" {
        // Just reset to HEAD to discard any current changes
        let output = Command::new("git")
            .args(["reset", "--hard", "HEAD"])
            .output()
            .context("Failed to run git reset --hard")?;
        if !output.status.success() {
            anyhow::bail!(
                "git reset failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return Ok("Restored clean state".to_string());
    }

    // First reset hard to clear current changes
    let _ = Command::new("git")
        .args(["reset", "--hard", "HEAD"])
        .output();

    // Apply the stash
    let output = Command::new("git")
        .args(["stash", "apply", stash_ref])
        .output()
        .context("Failed to run git stash apply")?;

    if !output.status.success() {
        anyhow::bail!(
            "git stash apply failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(format!("Restored from {}", stash_ref))
}

/// Create a named checkpoint using a temp branch.
/// This is more durable than stash for long-lived checkpoints.
pub fn branch_checkpoint(name: &str) -> Result<String> {
    let branch_name = format!("openshark-checkpoint/{}", name);
    let output = Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .output()
        .context("Failed to create checkpoint branch")?;

    if !output.status.success() {
        anyhow::bail!(
            "git checkout -b failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Commit current changes
    let stage = Command::new("git")
        .args(["add", "-A"])
        .output()
        .context("Failed to stage changes")?;
    let _ = stage;

    let commit = Command::new("git")
        .args([
            "commit",
            "-m",
            &format!("openshark checkpoint: {}", name),
            "--no-verify",
        ])
        .output()
        .context("Failed to commit checkpoint")?;

    if !commit.status.success() {
        // Might be nothing to commit — that's fine
        let stderr = String::from_utf8_lossy(&commit.stderr);
        if !stderr.contains("nothing to commit") && !stderr.contains("no changes added") {
            // Try to go back to original branch
            let _ = Command::new("git").args(["checkout", "-"]).output();
            anyhow::bail!("git commit failed: {}", stderr);
        }
    }

    // Go back to original branch
    let back = Command::new("git")
        .args(["checkout", "-"])
        .output()
        .context("Failed to switch back to original branch")?;

    if !back.status.success() {
        anyhow::bail!(
            "git checkout - failed: {}",
            String::from_utf8_lossy(&back.stderr)
        );
    }

    Ok(branch_name)
}

/// Restore from a checkpoint branch.
pub fn restore_branch(branch_name: &str) -> Result<String> {
    // Get current branch name first
    let current = Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .context("Failed to get current branch")?;
    let current_branch = String::from_utf8_lossy(&current.stdout).trim().to_string();

    // Reset hard
    let _ = Command::new("git")
        .args(["reset", "--hard", "HEAD"])
        .output();

    // Merge the checkpoint branch
    let output = Command::new("git")
        .args(["merge", "--ff-only", branch_name])
        .output()
        .context("Failed to merge checkpoint branch")?;

    if !output.status.success() {
        // Try reset to the branch instead
        let reset = Command::new("git")
            .args(["reset", "--hard", branch_name])
            .output()
            .context("Failed to reset to checkpoint branch")?;
        if !reset.status.success() {
            // Go back to original branch if we switched somehow
            let _ = Command::new("git")
                .args(["checkout", &current_branch])
                .output();
            anyhow::bail!(
                "Failed to restore checkpoint branch {}: {}",
                branch_name,
                String::from_utf8_lossy(&reset.stderr)
            );
        }
    }

    Ok(format!("Restored from branch {}", branch_name))
}

/// Check if we're inside a git repo.
pub fn in_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Save a checkpoint — prefers stash for speed, falls back to branch.
pub fn save_checkpoint(name: &str) -> Result<Checkpoint> {
    if !in_git_repo() {
        anyhow::bail!("Not in a git repository — checkpoints require git");
    }

    // Try stash first (fast, lightweight)
    match stash_checkpoint(name) {
        Ok(git_ref) => Ok(Checkpoint::new(name, git_ref, true)),
        Err(_) => {
            // Fall back to branch checkpoint
            let branch_name = branch_checkpoint(name)?;
            Ok(Checkpoint::new(name, branch_name, false))
        }
    }
}

/// Restore a checkpoint by its git ref.
pub fn restore_checkpoint(checkpoint: &Checkpoint) -> Result<String> {
    if checkpoint.is_stash {
        restore_stash(&checkpoint.git_ref)
    } else {
        restore_branch(&checkpoint.git_ref)
    }
}

/// Drop a stash ref.
#[allow(dead_code)]
pub fn drop_stash(stash_ref: &str) -> Result<()> {
    if stash_ref == "clean" {
        return Ok(());
    }
    let output = Command::new("git")
        .args(["stash", "drop", stash_ref])
        .output()
        .context("Failed to drop stash")?;
    if !output.status.success() {
        anyhow::bail!(
            "git stash drop failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Drop a checkpoint branch.
#[allow(dead_code)]
pub fn drop_branch(branch_name: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "-D", branch_name])
        .output()
        .context("Failed to delete checkpoint branch")?;
    if !output.status.success() {
        anyhow::bail!(
            "git branch -D failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Delete a checkpoint's git ref.
#[allow(dead_code)]
pub fn delete_checkpoint(checkpoint: &Checkpoint) -> Result<()> {
    if checkpoint.is_stash {
        drop_stash(&checkpoint.git_ref)
    } else {
        drop_branch(&checkpoint.git_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use std::sync::Mutex;

    // Serialize checkpoint tests — they mutate process-global cwd and git stash state.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn temp_git_repo() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = format!("/tmp/openshark_cp_test_{}_{}", std::process::id(), count);
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
        // Create initial commit so we have a HEAD
        fs::write(format!("{}/init.txt", dir), "init").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init", "--no-verify"])
            .current_dir(&dir)
            .output()
            .unwrap();
        dir
    }

    fn cleanup(dir: &str) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_checkpoint_stack() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut stack = CheckpointStack::new("test-session");
        assert_eq!(stack.undo_len(), 0);
        assert_eq!(stack.redo_len(), 0);

        stack.push(Checkpoint::new("cp1", "stash@{0}", true));
        assert_eq!(stack.undo_len(), 1);

        let cp = stack.pop_undo().unwrap();
        assert_eq!(cp.name, "cp1");
        stack.push_redo(cp);
        assert_eq!(stack.redo_len(), 1);
    }

    #[test]
    fn test_in_git_repo() {
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = temp_git_repo();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        assert!(in_git_repo());
        std::env::set_current_dir(original).unwrap();
        cleanup(&dir);
    }

    #[test]
    fn test_stash_checkpoint_clean_repo() {
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = temp_git_repo();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();

        // Clean repo — stash should return "clean"
        let result = stash_checkpoint("test-clean");
        assert!(result.is_ok());
        let git_ref = result.unwrap();
        assert_eq!(git_ref, "clean");

        std::env::set_current_dir(original).unwrap();
        cleanup(&dir);
    }

    #[test]
    fn test_stash_checkpoint_with_changes() {
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = temp_git_repo();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();

        fs::write(format!("{}/changed.txt", dir), "new content").unwrap();
        let result = stash_checkpoint("test-changes");
        assert!(result.is_ok());

        // File should be gone after stash
        assert!(!std::path::Path::new(&format!("{}/changed.txt", dir)).exists());

        // Restore
        let git_ref = result.unwrap();
        let restore_result = restore_stash(&git_ref);
        assert!(restore_result.is_ok());
        assert!(std::path::Path::new(&format!("{}/changed.txt", dir)).exists());

        std::env::set_current_dir(original).unwrap();
        cleanup(&dir);
    }
}
