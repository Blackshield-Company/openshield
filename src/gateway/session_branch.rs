//! Session branching — fork conversation state into named branches.
//!
//! Branches are ephemeral (in-memory) snapshots of a channel's conversation state.
//! They allow users to explore alternative conversation paths without losing the
//! original thread. Branches are per-channel and expire when the process restarts.
//!
//! Usage:
//!   !branch save `<name>`     — Save current state as a named branch
//!   !branch load `<name>`     — Restore a branch to the current channel
//!   !branch list            — Show all branches for this channel
//!   !branch delete `<name>`   — Delete a branch
//!   !branch diff `<name>`     — Show diff between current and branch

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::gateway::channel_state::ChannelState;

/// A named snapshot of a channel's conversation state.
#[derive(Clone)]
pub struct Branch {
    #[allow(dead_code)]
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub state: ChannelState,
}

/// Thread-safe branch registry, keyed by (channel_id, branch_name).
#[derive(Clone)]
pub struct BranchRegistry {
    branches: Arc<Mutex<HashMap<(u64, String), Branch>>>,
}

impl BranchRegistry {
    pub fn new() -> Self {
        Self {
            branches: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Save the current channel state as a named branch.
    pub fn save(&self, channel_id: u64, name: &str, state: ChannelState) {
        let mut branches = self.branches.lock().expect("SessionBranchRegistry mutex poisoned");
        branches.insert(
            (channel_id, name.to_string()),
            Branch {
                name: name.to_string(),
                created_at: chrono::Utc::now(),
                state,
            },
        );
    }

    /// Load a branch back into a channel state.
    pub fn load(&self, channel_id: u64, name: &str) -> Option<ChannelState> {
        let branches = self.branches.lock().expect("SessionBranchRegistry mutex poisoned");
        branches
            .get(&(channel_id, name.to_string()))
            .map(|b| b.state.clone())
    }

    /// List all branch names for a channel.
    pub fn list(&self, channel_id: u64) -> Vec<BranchInfo> {
        let branches = self.branches.lock().expect("SessionBranchRegistry mutex poisoned");
        branches
            .iter()
            .filter(|((cid, _), _)| *cid == channel_id)
            .map(|((_, name), branch)| BranchInfo {
                name: name.clone(),
                created_at: branch.created_at,
                message_count: branch.state.history.len().saturating_sub(1),
                model: branch.state.model.clone(),
            })
            .collect()
    }

    /// Delete a branch.
    pub fn delete(&self, channel_id: u64, name: &str) -> bool {
        let mut branches = self.branches.lock().expect("SessionBranchRegistry mutex poisoned");
        branches.remove(&(channel_id, name.to_string())).is_some()
    }

    /// Check if a branch exists.
    #[allow(dead_code)]
    pub fn exists(&self, channel_id: u64, name: &str) -> bool {
        let branches = self.branches.lock().expect("SessionBranchRegistry mutex poisoned");
        branches.contains_key(&(channel_id, name.to_string()))
    }
}

/// Lightweight branch metadata for listing.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub model: String,
}

/// Compute a human-readable diff between two channel states.
pub fn diff_states(current: &ChannelState, branch: &ChannelState) -> String {
    let mut lines = Vec::new();

    // Model diff
    if current.model != branch.model {
        lines.push(format!("Model: `{}` → `{}`", branch.model, current.model));
    }

    // History length diff
    let current_msgs = current.history.len().saturating_sub(1);
    let branch_msgs = branch.history.len().saturating_sub(1);
    if current_msgs != branch_msgs {
        let delta = current_msgs as isize - branch_msgs as isize;
        let sign = if delta > 0 { "+" } else { "" };
        lines.push(format!(
            "Messages: {} → {} ({}{})",
            branch_msgs, current_msgs, sign, delta
        ));
    }

    // System prompt diff
    let current_prompt = current
        .history
        .first()
        .map(|m| m.content.as_str())
        .unwrap_or("");
    let branch_prompt = branch
        .history
        .first()
        .map(|m| m.content.as_str())
        .unwrap_or("");
    if current_prompt != branch_prompt {
        lines.push("System prompt: changed".to_string());
    }

    // Multi-model diff
    if current.multi_model_enabled != branch.multi_model_enabled {
        lines.push(format!(
            "Multi-model: {} → {}",
            branch.multi_model_enabled, current.multi_model_enabled
        ));
    }

    if lines.is_empty() {
        "No differences — branch is identical to current state.".to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_config() -> Config {
        Config::default()
    }

    #[test]
    fn test_branch_save_and_load() {
        let registry = BranchRegistry::new();
        let config = test_config();
        let state = ChannelState::new(&config);

        registry.save(1, "main", state.clone());
        assert!(registry.exists(1, "main"));

        let loaded = registry.load(1, "main").unwrap();
        assert_eq!(loaded.model, state.model);
    }

    #[test]
    fn test_branch_list() {
        let registry = BranchRegistry::new();
        let config = test_config();
        let state = ChannelState::new(&config);

        registry.save(1, "v1", state.clone());
        registry.save(1, "v2", state.clone());
        registry.save(2, "other", state.clone());

        let list = registry.list(1);
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|b| b.name == "v1"));
        assert!(list.iter().any(|b| b.name == "v2"));
    }

    #[test]
    fn test_branch_delete() {
        let registry = BranchRegistry::new();
        let config = test_config();
        let state = ChannelState::new(&config);

        registry.save(1, "temp", state);
        assert!(registry.delete(1, "temp"));
        assert!(!registry.exists(1, "temp"));
        assert!(!registry.delete(1, "temp"));
    }

    #[test]
    fn test_branch_isolation() {
        let registry = BranchRegistry::new();
        let config = test_config();
        let mut state1 = ChannelState::new(&config);
        state1.model = "model-a".to_string();
        let mut state2 = ChannelState::new(&config);
        state2.model = "model-b".to_string();

        registry.save(1, "a", state1);
        registry.save(1, "b", state2);

        let loaded_a = registry.load(1, "a").unwrap();
        let loaded_b = registry.load(1, "b").unwrap();
        assert_eq!(loaded_a.model, "model-a");
        assert_eq!(loaded_b.model, "model-b");
    }

    #[test]
    fn test_diff_states() {
        let config = test_config();
        let mut current = ChannelState::new(&config);
        let branch = current.clone();

        current.model = "new-model".to_string();
        current.add_user_message("hello".to_string());

        let diff = diff_states(&current, &branch);
        assert!(diff.contains("new-model"));
        assert!(diff.contains("Messages:"));
    }
}
