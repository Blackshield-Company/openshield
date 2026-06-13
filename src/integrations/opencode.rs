//! OpenCode Delegation — Optional spawn-and-stream integration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpencodeConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

impl Default for OpencodeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_seconds: default_timeout(),
        }
    }
}

fn default_false() -> bool {
    false
}
fn default_timeout() -> u64 {
    300
}

pub fn detect() -> bool {
    std::process::Command::new("opencode")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn delegate(task: &str, _timeout: u64) -> anyhow::Result<String> {
    if !detect() {
        anyhow::bail!("OpenCode not installed. Install: npm install -g opencode");
    }

    let output = std::process::Command::new("opencode")
        .args(["task", task])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    Ok(format!(
        "OpenCode output:\n{}\n[stderr]: {}",
        stdout, stderr
    ))
}
