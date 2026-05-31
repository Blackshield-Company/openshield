use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

pub mod consensus;
pub mod roles;
pub mod agent_runner;

use consensus::{ConsensusMemory, ConsensusEntry};
use roles::{AgentRole, RoleTemplate};
use crate::config::Config;
use crate::providers::Provider;

/// Unique identifier for an agent in the swarm.
pub type AgentId = String;

/// Status of an agent in the swarm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentStatus {
    Idle,
    Working { task: String, started_at: u64 },
    Reviewing { target_agent: AgentId, started_at: u64 },
    WaitingForConsensus { started_at: u64 },
    Error { message: String },
    Completed { result: String },
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Idle => write!(f, "idle"),
            AgentStatus::Working { task, .. } => write!(f, "working on: {}", task),
            AgentStatus::Reviewing { target_agent, .. } => write!(f, "reviewing {}", target_agent),
            AgentStatus::WaitingForConsensus { .. } => write!(f, "waiting for consensus"),
            AgentStatus::Error { message } => write!(f, "error: {}", message),
            AgentStatus::Completed { result } => write!(f, "completed: {}", result),
        }
    }
}

/// A single agent in the swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmAgent {
    pub id: AgentId,
    pub name: String,
    pub role: AgentRole,
    pub status: AgentStatus,
    pub cycles_completed: usize,
    pub errors_count: usize,
    pub success_rate: f64,
    pub last_activity: Option<String>,
    pub model: String,
}

/// Event types for inter-agent communication.
#[derive(Debug, Clone)]
pub enum SwarmEvent {
    /// An agent completed work and wants consensus.
    WorkCompleted {
        agent_id: AgentId,
        task: String,
        result: String,
    },
    /// An agent is requesting review from another agent.
    ReviewRequested {
        from_agent: AgentId,
        to_agent: AgentId,
        content: String,
    },
    /// A review was completed.
    ReviewCompleted {
        reviewer: AgentId,
        target_agent: AgentId,
        approval: bool,
        feedback: String,
    },
    /// Consensus was reached on an entry.
    ConsensusReached {
        entry_id: String,
        approved_by: Vec<AgentId>,
    },
    /// An agent encountered an error.
    AgentError {
        agent_id: AgentId,
        error: String,
    },
    /// A heartbeat from an agent.
    Heartbeat {
        agent_id: AgentId,
        timestamp: u64,
    },
    /// Shutdown signal.
    Shutdown,
}

/// Configuration for swarm mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max_agents")]
    pub max_agents: usize,
    #[serde(default = "default_consensus_required")]
    pub consensus_required: bool,
    #[serde(default = "default_consensus_mode")]
    pub consensus_mode: String,
    #[serde(default = "default_cycle_limit")]
    pub cycle_limit: usize,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default = "default_auto_spawn")]
    pub auto_spawn: bool,
}

fn default_max_agents() -> usize { 8 }
fn default_consensus_required() -> bool { true }
fn default_consensus_mode() -> String { "majority".to_string() }
fn default_cycle_limit() -> usize { 50 }
fn default_auto_spawn() -> bool { false }

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_agents: 8,
            consensus_required: true,
            consensus_mode: "majority".to_string(),
            cycle_limit: 50,
            roles: vec![
                "architect".to_string(),
                "implementer".to_string(),
                "reviewer".to_string(),
                "tester".to_string(),
            ],
            auto_spawn: false,
        }
    }
}

/// The swarm engine manages multiple agents working together.
pub struct SwarmEngine {
    config: SwarmConfig,
    agents: Arc<RwLock<HashMap<AgentId, SwarmAgent>>>,
    runners: Arc<RwLock<HashMap<AgentId, Arc<agent_runner::AgentRunner>>>>,
    consensus: Arc<Mutex<ConsensusMemory>>,
    event_tx: mpsc::UnboundedSender<SwarmEvent>,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<SwarmEvent>>>,
    running: Arc<RwLock<bool>>,
    cycle_count: Arc<RwLock<usize>>,
    seed_prompt: Arc<RwLock<String>>,
}

impl SwarmEngine {
    /// Create a new swarm engine with the given configuration.
    pub fn new(config: SwarmConfig) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let consensus = ConsensusMemory::new(&config.consensus_mode);

        Self {
            config,
            agents: Arc::new(RwLock::new(HashMap::new())),
            runners: Arc::new(RwLock::new(HashMap::new())),
            consensus: Arc::new(Mutex::new(consensus)),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            running: Arc::new(RwLock::new(false)),
            cycle_count: Arc::new(RwLock::new(0)),
            seed_prompt: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Initialize the swarm with a seed prompt and spawn initial agents.
    pub async fn init(&self, seed_prompt: &str, global_config: &Config) -> Result<()> {
        info!("🐝 Initializing swarm with prompt: {}", seed_prompt.chars().take(60).collect::<String>());

        *self.seed_prompt.write().await = seed_prompt.to_string();

        // Clear any existing agents and runners
        self.agents.write().await.clear();
        self.runners.write().await.clear();

        // Build provider for agents
        let provider = agent_runner::build_agent_provider(global_config)?;

        // Spawn agents for each configured role
        for (i, role_name) in self.config.roles.iter().enumerate() {
            if i >= self.config.max_agents {
                warn!("Max agents ({}) reached, skipping remaining roles", self.config.max_agents);
                break;
            }

            let role = RoleTemplate::get(role_name).unwrap_or_else(|| RoleTemplate::default_role());
            let agent_id = format!("{}-{}", role.short_name(), i + 1);
            let model = global_config.default_model.clone();

            let agent = SwarmAgent {
                id: agent_id.clone(),
                name: format!("{} {}", role.name(), i + 1),
                role: role.to_agent_role(),
                status: AgentStatus::Idle,
                cycles_completed: 0,
                errors_count: 0,
                success_rate: 1.0,
                last_activity: None,
                model: model.clone(),
            };

            // Build system prompt from role + seed
            let system_prompt = format!(
                "{}\n\nYou are part of a multi-agent swarm working on: {}\n\
                 You have access to tools. When you need to use a tool, output it as: TOOL:<tool_name> <args>\n\
                 Be concise and direct. Focus on your specific role.",
                role.to_agent_role().system_prompt_addendum,
                seed_prompt
            );

            let runner = Arc::new(agent_runner::AgentRunner::new(
                agent_id.clone(),
                provider.clone(),
                model,
                self.event_tx.clone(),
                &system_prompt,
            ));

            self.agents.write().await.insert(agent_id.clone(), agent);
            self.runners.write().await.insert(agent_id, runner);
            info!("  ✓ Spawned agent: {}", role.name());
        }

        info!("🐝 Swarm initialized with {} agents", self.agents.read().await.len());
        Ok(())
    }

    /// Start the autonomous swarm loop.
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        info!("🐝 Swarm loop starting...");

        // Kick off initial tasks for each agent based on their role
        let seed = self.seed_prompt.read().await.clone();
        let runners = self.runners.read().await.clone();
        let agents = self.agents.clone();

        for (agent_id, runner) in runners.iter() {
            let agent_id = agent_id.clone();
            let runner = runner.clone();
            let seed = seed.clone();
            let agents = agents.clone();

            tokio::spawn(async move {
                // Determine task based on role
                let task = {
                    let agents_lock = agents.read().await;
                    if let Some(agent) = agents_lock.get(&agent_id) {
                        match agent.role.name.as_str() {
                            "Architect" => format!("Design the system architecture for: {}", seed),
                            "Implementer" => format!("Implement the core functionality for: {}", seed),
                            "Reviewer" => format!("Review the approach for: {}. What are the risks and improvements needed?", seed),
                            "Tester" => format!("Design test cases and verify requirements for: {}", seed),
                            "DevOps" => format!("Design deployment and CI/CD strategy for: {}", seed),
                            "Security" => format!("Perform security audit of the design for: {}", seed),
                            "Documentation" => format!("Write documentation outline for: {}", seed),
                            "Project Manager" => format!("Break down the project into tasks and milestones for: {}", seed),
                            _ => format!("Analyze and provide insights on: {}", seed),
                        }
                    } else {
                        seed.clone()
                    }
                };

                let agent_ref = {
                    let agents_lock = agents.read().await;
                    if let Some(agent) = agents_lock.get(&agent_id) {
                        Arc::new(RwLock::new(agent.clone()))
                    } else {
                        return;
                    }
                };

                match runner.execute_task(&task, &agent_ref).await {
                    Ok(result) => {
                        info!("🐝 Agent {} completed initial task ({} chars)", agent_id, result.len());
                    }
                    Err(e) => {
                        warn!("🐝 Agent {} failed initial task: {}", agent_id, e);
                    }
                }
            });
        }

        let event_rx = self.event_rx.clone();
        let event_tx = self.event_tx.clone();
        let agents = self.agents.clone();
        let consensus = self.consensus.clone();
        let running_flag = self.running.clone();
        let cycle_count = self.cycle_count.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut rx = event_rx.lock().await;

            while *running_flag.read().await {
                match rx.recv().await {
                    Some(SwarmEvent::WorkCompleted { agent_id, task, result }) => {
                        debug!("Agent {} completed work on: {}", agent_id, task);

                        // Update agent status
                        if let Some(agent) = agents.write().await.get_mut(&agent_id) {
                            agent.status = AgentStatus::WaitingForConsensus {
                                started_at: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                            };
                            agent.cycles_completed += 1;
                            agent.last_activity = Some(format!("Completed: {}", task));
                        }

                        // Add to consensus memory
                        let entry = ConsensusEntry {
                            id: format!("entry-{}", uuid::Uuid::new_v4()),
                            author: agent_id.clone(),
                            task: task.clone(),
                            content: result.clone(),
                            approvals: vec![agent_id.clone()],
                            rejections: vec![],
                            status: consensus::EntryStatus::Pending,
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                        };

                        if let Ok(mut cons) = consensus.try_lock() {
                            cons.add_entry(entry);
                        }

                        // Trigger cross-agent review: find a Reviewer agent and assign review
                        let reviewer_id = {
                            let agents_lock = agents.read().await;
                            agents_lock.iter()
                                .find(|(_, a)| a.role.name == "Reviewer" && a.id != agent_id)
                                .map(|(id, _)| id.clone())
                        };

                        if let Some(reviewer_id) = reviewer_id {
                            info!("🐝 Assigning review of {} to {}", agent_id, reviewer_id);
                            let _ = event_tx.send(SwarmEvent::ReviewRequested {
                                from_agent: agent_id.clone(),
                                to_agent: reviewer_id,
                                content: result.clone(),
                            });
                        }

                        // Increment cycle count
                        *cycle_count.write().await += 1;

                        // Check cycle limit
                        if *cycle_count.read().await >= config.cycle_limit {
                            info!("🐝 Swarm cycle limit ({}) reached, stopping", config.cycle_limit);
                            *running_flag.write().await = false;
                        }
                    }

                    Some(SwarmEvent::ReviewRequested { from_agent, to_agent, content }) => {
                        debug!("Review requested from {} to {}", from_agent, to_agent);
                        if let Some(agent) = agents.write().await.get_mut(&to_agent) {
                            agent.status = AgentStatus::Reviewing {
                                target_agent: from_agent.clone(),
                                started_at: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                            };
                            agent.last_activity = Some(format!("Reviewing {}'s work", from_agent));
                        }
                    }

                    Some(SwarmEvent::ReviewCompleted { reviewer, target_agent, approval, feedback }) => {
                        debug!("Review completed by {} for {}: approved={}", reviewer, target_agent, approval);

                        if let Some(agent) = agents.write().await.get_mut(&reviewer) {
                            agent.status = AgentStatus::Idle;
                            agent.last_activity = Some(format!("Reviewed {}: {}", target_agent,
                                if approval { "approved" } else { "rejected" }));
                        }

                        // Update consensus entry
                        if let Ok(mut cons) = consensus.try_lock() {
                            if approval {
                                cons.approve_entry(&target_agent, &reviewer);
                            } else {
                                cons.reject_entry(&target_agent, &reviewer, &feedback);
                            }
                        }
                    }

                    Some(SwarmEvent::AgentError { agent_id, error }) => {
                        warn!("Agent {} error: {}", agent_id, error);
                        if let Some(agent) = agents.write().await.get_mut(&agent_id) {
                            agent.status = AgentStatus::Error { message: error.clone() };
                            agent.errors_count += 1;
                            agent.success_rate = agent.cycles_completed as f64
                                / (agent.cycles_completed + agent.errors_count).max(1) as f64;
                            agent.last_activity = Some(format!("Error: {}", error));
                        }
                    }

                    Some(SwarmEvent::Heartbeat { agent_id, timestamp }) => {
                        debug!("Heartbeat from {} at {}", agent_id, timestamp);
                    }

                    Some(SwarmEvent::ConsensusReached { entry_id, approved_by }) => {
                        debug!("Consensus reached on {} by {:?}", entry_id, approved_by);
                    }

                    Some(SwarmEvent::Shutdown) => {
                        info!("🐝 Swarm shutdown requested");
                        *running_flag.write().await = false;
                    }

                    None => {
                        // Channel closed
                        break;
                    }
                }
            }

            info!("🐝 Swarm loop stopped");
        });

        Ok(())
    }

    /// Stop the swarm loop.
    pub async fn stop(&self) -> Result<()> {
        info!("🐝 Stopping swarm...");
        let _ = self.event_tx.send(SwarmEvent::Shutdown);
        *self.running.write().await = false;

        // Set all agents to idle
        for (_, agent) in self.agents.write().await.iter_mut() {
            agent.status = AgentStatus::Idle;
        }

        Ok(())
    }

    /// Get a snapshot of all agents.
    pub async fn agent_snapshot(&self) -> Vec<SwarmAgent> {
        self.agents.read().await.values().cloned().collect()
    }

    /// Get the consensus memory contents.
    pub async fn consensus_snapshot(&self) -> Vec<ConsensusEntry> {
        self.consensus.lock().await.entries()
    }

    /// Get swarm status summary.
    pub async fn status(&self) -> SwarmStatus {
        let agents = self.agents.read().await;
        let total = agents.len();
        let working = agents.values().filter(|a| matches!(a.status, AgentStatus::Working { .. })).count();
        let idle = agents.values().filter(|a| matches!(a.status, AgentStatus::Idle)).count();
        let errors = agents.values().filter(|a| matches!(a.status, AgentStatus::Error { .. })).count();
        let cycles = *self.cycle_count.read().await;
        let running = *self.running.read().await;

        SwarmStatus {
            running,
            total_agents: total,
            working_agents: working,
            idle_agents: idle,
            error_agents: errors,
            cycles_completed: cycles,
            cycle_limit: self.config.cycle_limit,
            consensus_entries: self.consensus.lock().await.entry_count(),
        }
    }

    /// Send an event into the swarm.
    pub fn send_event(&self, event: SwarmEvent) -> Result<()> {
        self.event_tx.send(event)
            .context("Swarm event channel closed")?;
        Ok(())
    }

    /// Check if the swarm is running.
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }
}

/// Summary status of the swarm.
#[derive(Debug, Clone, Serialize)]
pub struct SwarmStatus {
    pub running: bool,
    pub total_agents: usize,
    pub working_agents: usize,
    pub idle_agents: usize,
    pub error_agents: usize,
    pub cycles_completed: usize,
    pub cycle_limit: usize,
    pub consensus_entries: usize,
}

impl std::fmt::Display for SwarmStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "🐝 Swarm Status")?;
        writeln!(f, "  Running: {}", if self.running { "✅" } else { "⏹️" })?;
        writeln!(f, "  Agents: {} total, {} working, {} idle, {} errors",
            self.total_agents, self.working_agents, self.idle_agents, self.error_agents)?;
        writeln!(f, "  Cycles: {}/{}", self.cycles_completed, self.cycle_limit)?;
        writeln!(f, "  Consensus entries: {}", self.consensus_entries)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_swarm_init() {
        let config = SwarmConfig::default();
        let engine = SwarmEngine::new(config);

        let test_config = crate::config::Config::default();
        engine.init("Build a REST API", &test_config).await.unwrap();

        let agents = engine.agent_snapshot().await;
        assert!(!agents.is_empty());
        assert_eq!(agents.len(), 4); // Default roles
    }

    #[tokio::test]
    async fn test_swarm_start_stop() {
        let config = SwarmConfig::default();
        let engine = SwarmEngine::new(config);

        let test_config = crate::config::Config::default();
        engine.init("Test", &test_config).await.unwrap();

        assert!(!engine.is_running().await);

        engine.start().await.unwrap();
        assert!(engine.is_running().await);

        engine.stop().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert!(!engine.is_running().await);
    }

    #[tokio::test]
    async fn test_swarm_events() {
        let config = SwarmConfig::default();
        let engine = SwarmEngine::new(config);

        let test_config = crate::config::Config::default();
        engine.init("Test", &test_config).await.unwrap();
        engine.start().await.unwrap();

        // Send a work completed event
        engine.send_event(SwarmEvent::WorkCompleted {
            agent_id: "architect-1".to_string(),
            task: "Design API".to_string(),
            result: "API designed".to_string(),
        }).unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let status = engine.status().await;
        assert_eq!(status.cycles_completed, 1);
        assert_eq!(status.consensus_entries, 1);

        engine.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_swarm_status_display() {
        let status = SwarmStatus {
            running: true,
            total_agents: 4,
            working_agents: 2,
            idle_agents: 1,
            error_agents: 1,
            cycles_completed: 5,
            cycle_limit: 50,
            consensus_entries: 3,
        };

        let output = format!("{}", status);
        assert!(output.contains("Swarm Status"));
        assert!(output.contains("4 total"));
    }
}
