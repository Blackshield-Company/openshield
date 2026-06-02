//! Unified Delegation Registry
//! 
//! Routes tasks to available external agents. All optional.

use super::{claw, claude, opencode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claw,
    OpenCode,
    Claude,
}

impl std::fmt::Display for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Agent::Claw => write!(f, "claw"),
            Agent::OpenCode => write!(f, "opencode"),
            Agent::Claude => write!(f, "claude"),
        }
    }
}

impl std::str::FromStr for Agent {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claw" => Ok(Agent::Claw),
            "opencode" | "open-code" => Ok(Agent::OpenCode),
            "claude" | "claude-code" => Ok(Agent::Claude),
            _ => Err(format!("Unknown agent: {}", s)),
        }
    }
}

/// List all available (detected) agents.
pub fn available() -> Vec<Agent> {
    let mut agents = Vec::new();
    if claw::detect() { agents.push(Agent::Claw); }
    if opencode::detect() { agents.push(Agent::OpenCode); }
    if claude::detect() { agents.push(Agent::Claude); }
    agents
}

/// Delegate a task to a specific agent.
pub fn delegate(agent: Agent, task: &str, timeout: u64) -> anyhow::Result<String> {
    match agent {
        Agent::Claw => claw::delegate(task, timeout),
        Agent::OpenCode => opencode::delegate(task, timeout),
        Agent::Claude => claude::delegate(task, timeout),
    }
}

/// Auto-delegate to the first available agent.
pub fn auto_delegate(task: &str, timeout: u64) -> anyhow::Result<String> {
    let agents = available();
    if agents.is_empty() {
        anyhow::bail!("No external agents detected. Install claw, opencode, or claude-code.");
    }
    delegate(agents[0], task, timeout)
}
