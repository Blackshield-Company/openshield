//! Hermes Bridge — Optional two-way sync with Hermes Agent.
//!
//! Features: `hermes-bridge` (compile-time) + config gate (runtime)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default = "default_hermes_home")]
    pub hermes_home: String,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_seconds: u64,
    #[serde(default = "default_true")]
    pub pull_memories: bool,
    #[serde(default = "default_false")]
    pub push_skills: bool,
}

impl Default for HermesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hermes_home: default_hermes_home(),
            sync_interval_seconds: default_sync_interval(),
            pull_memories: true,
            push_skills: false,
        }
    }
}

fn default_false() -> bool {
    false
}
fn default_true() -> bool {
    true
}
fn default_hermes_home() -> String {
    "~/.hermes".to_string()
}
fn default_sync_interval() -> u64 {
    300
}

/// Check if Hermes is installed and accessible.
pub fn detect() -> bool {
    std::process::Command::new("hermes")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Pull memories from Hermes into OpenShark.
pub fn sync_pull(hermes_home: &str) -> anyhow::Result<String> {
    let expanded = shellexpand::tilde(hermes_home);
    let memory_src = std::path::Path::new(expanded.as_ref()).join("memory");

    if !memory_src.exists() {
        anyhow::bail!(
            "Hermes memory directory not found: {}",
            memory_src.display()
        );
    }

    // TODO: Implement actual memory sync
    anyhow::bail!("Not yet implemented — Hermes sync is a planned feature")
}

/// Push OpenShark skills to Hermes.
pub fn sync_push(hermes_home: &str) -> anyhow::Result<String> {
    let expanded = shellexpand::tilde(hermes_home);
    let skills_dst = std::path::Path::new(expanded.as_ref()).join("skills");

    if !skills_dst.exists() {
        anyhow::bail!(
            "Hermes skills directory not found: {}",
            skills_dst.display()
        );
    }

    // TODO: Implement actual skills sync
    anyhow::bail!("Not yet implemented — Hermes sync is a planned feature")
}
