//! Claude Code Delegation — Optional spawn-and-stream integration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_seconds: default_timeout(),
        }
    }
}

fn default_false() -> bool { false }
fn default_timeout() -> u64 { 300 }

pub fn detect() -> bool {
    std::process::Command::new("claude")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn delegate(task: &str, timeout: u64) -> anyhow::Result<String> {
    if !detect() {
        anyhow::bail!("Claude Code not installed. Install: npm install -g @anthropic-ai/claude-code");
    }
    
    let output = std::process::Command::new("claude")
        .args(["-p", task])
        
        .output()?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    Ok(format!("Claude Code output:\n{}\n[stderr]: {}", stdout, stderr))
}
