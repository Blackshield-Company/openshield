//! OpenShark Self-Evolution System
//!
//! This module wires together memory, skills, routing, and self-improvement
//! to create an agent that learns from every interaction and adapts its behavior.
//!
//! ## Architecture
//!
//! ```
//! User Message
//!     │
//!     ├─→ Memory Recall (inject relevant past context)
//!     ├─→ Skill Triggering (load relevant skills)
//!     ├─→ Model Routing (select best model based on historical performance)
//!     │
//!     ▼
//! Model Response
//!     │
//!     ├─→ Tool Execution (with security gate)
//!     ├─→ Performance Tracking (latency, success/failure)
//!     │
//!     ▼
//! Feedback Loop
//!     ├─→ Update routing weights
//!     ├─→ Update tool confidence thresholds
//!     ├─→ Trigger self-analysis every N sessions
//!     └─→ Auto-create skills from patterns
//! ```

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::config::Config;
use crate::memory::{ContextInjector, MemoryStore};
use crate::skills::{Skill, SkillRegistry, format_skills_prompt};

/// Tracks adaptive parameters that evolve based on performance data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdaptiveState {
    /// Tool confidence thresholds: tool_name -> minimum confidence to auto-execute
    pub tool_confidence: HashMap<String, f32>,
    /// Model routing bias: model_name -> performance multiplier
    pub model_bias: HashMap<String, f64>,
    /// Session count since last self-analysis
    pub sessions_since_analysis: usize,
    /// Total sessions tracked
    pub total_sessions: usize,
    /// Whether auto-analysis is enabled
    pub auto_analysis_enabled: bool,
    /// Analysis trigger threshold (sessions)
    pub analysis_threshold: usize,
}

impl Default for AdaptiveState {
    fn default() -> Self {
        let mut tool_confidence = HashMap::new();
        tool_confidence.insert("fs".to_string(), 0.7);
        tool_confidence.insert("terminal".to_string(), 0.8);
        tool_confidence.insert("git".to_string(), 0.6);
        tool_confidence.insert("search".to_string(), 0.6);
        tool_confidence.insert("edit".to_string(), 0.9);
        tool_confidence.insert("lsp".to_string(), 0.7);
        tool_confidence.insert("refactor".to_string(), 0.9);
        tool_confidence.insert("test".to_string(), 0.7);

        Self {
            tool_confidence,
            model_bias: HashMap::new(),
            sessions_since_analysis: 0,
            total_sessions: 0,
            auto_analysis_enabled: true,
            analysis_threshold: 20,
        }
    }
}

/// The central evolution engine that coordinates all adaptive behavior.
pub struct EvolutionEngine {
    /// Persistent memory store
    pub memory: Arc<Mutex<MemoryStore>>,
    /// Skill registry for trigger-based loading
    pub skill_registry: Arc<Mutex<SkillRegistry>>,
    /// Adaptive state (confidence thresholds, biases)
    pub adaptive_state: Arc<Mutex<AdaptiveState>>,
    /// User config
    pub config: Config,
}

impl EvolutionEngine {
    /// Create a new evolution engine
    pub fn new(config: &Config) -> Result<Self> {
        let memory = Arc::new(Mutex::new(
            MemoryStore::new(&config.memory_db_path).context("Failed to open memory store")?,
        ));

        let skills_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("openshark")
            .join("skills")
            .join("user");

        let skill_registry = Arc::new(Mutex::new(
            SkillRegistry::new(skills_dir).context("Failed to load skill registry")?,
        ));

        // Load adaptive state from memory or use defaults
        let adaptive_state = Arc::new(Mutex::new(
            Self::load_adaptive_state(&memory.lock().unwrap()).unwrap_or_default(),
        ));

        Ok(Self {
            memory,
            skill_registry,
            adaptive_state,
            config: config.clone(),
        })
    }

    /// Build the enriched system prompt for a message.
    ///
    /// This is the core of Phase 1 — it injects:
    /// 1. Relevant memory context (past conversations about similar topics)
    /// 2. Triggered skills (procedural knowledge)
    /// 3. Filesystem capabilities
    /// 4. Tool availability
    pub fn build_enriched_prompt(
        &self,
        base_prompt: &str,
        user_message: &str,
        session_id: &str,
    ) -> String {
        let mut enriched = base_prompt.to_string();

        // ── Phase 1a: Memory Recall ──────────────────────────────────────────
        let memory_context = self.recall_memory(user_message, session_id);
        if !memory_context.is_empty() {
            enriched.push_str("\n\n[MEMORY RECALL]\n");
            enriched.push_str(&memory_context);
            enriched.push_str("\n[END MEMORY]\n");
        }

        // ── Phase 1b: Skill Injection ────────────────────────────────────────
        let skills = self.load_skills(user_message);
        if !skills.is_empty() {
            enriched.push_str(&skills);
        }

        // ── Phase 2a: Adaptive Tool Guidance ─────────────────────────────────
        let tool_guidance = self.build_tool_guidance();
        if !tool_guidance.is_empty() {
            enriched.push_str("\n\n[ADAPTIVE TOOL GUIDANCE]\n");
            enriched.push_str(&tool_guidance);
            enriched.push_str("\n[END TOOL GUIDANCE]\n");
        }

        enriched
    }

    /// Recall relevant past context from memory.
    fn recall_memory(&self, query: &str, session_id: &str) -> String {
        let memory = match self.memory.lock() {
            Ok(m) => m,
            Err(_) => return String::new(),
        };

        let injector = ContextInjector::new(&memory);

        // Try semantic search first
        match injector.inject_relevant_context(query, session_id) {
            Ok(messages) if !messages.is_empty() => {
                let mut context = String::new();
                for (i, msg) in messages.iter().take(3).enumerate() {
                    let preview = if msg.content.len() > 300 {
                        format!("{}...", &msg.content[..300])
                    } else {
                        msg.content.clone()
                    };
                    context.push_str(&format!(
                        "{} [{}] {}: {}\n",
                        i + 1,
                        msg.created_at.format("%Y-%m-%d"),
                        msg.role,
                        preview
                    ));
                }
                context
            }
            _ => {
                // Fallback: try natural language query
                if let Ok(answer) = injector.answer_natural_query(query) {
                    if !answer.contains("couldn't find") {
                        return format!("From past conversations:\n{}", answer);
                    }
                }
                String::new()
            }
        }
    }

    /// Load triggered skills for the current query.
    fn load_skills(&self, query: &str) -> String {
        let registry = match self.skill_registry.lock() {
            Ok(r) => r,
            Err(_) => return String::new(),
        };

        let triggered = registry.find_triggered(query);
        if triggered.is_empty() {
            return String::new();
        }

        let refs: Vec<&Skill> = triggered.into_iter().collect();
        format_skills_prompt(&refs)
    }

    /// Build adaptive tool guidance based on historical performance.
    fn build_tool_guidance(&self) -> String {
        let state = match self.adaptive_state.lock() {
            Ok(s) => s,
            Err(_) => return String::new(),
        };

        let mut guidance = String::new();

        // High-confidence tools (auto-execute)
        let auto_tools: Vec<String> = state
            .tool_confidence
            .iter()
            .filter(|(_, threshold)| **threshold <= 0.7)
            .map(|(name, _)| name.clone())
            .collect();

        if !auto_tools.is_empty() {
            guidance.push_str(&format!(
                "Auto-execute tools (proven reliable): {}\n",
                auto_tools.join(", ")
            ));
        }

        // Medium-confidence tools (ask first)
        let ask_tools: Vec<String> = state
            .tool_confidence
            .iter()
            .filter(|(_, threshold)| **threshold > 0.7 && **threshold <= 0.85)
            .map(|(name, _)| name.clone())
            .collect();

        if !ask_tools.is_empty() {
            guidance.push_str(&format!("Confirm before using: {}\n", ask_tools.join(", ")));
        }

        // Model bias hints
        if !state.model_bias.is_empty() {
            guidance.push_str("\nModel performance notes:\n");
            for (model, bias) in &state.model_bias {
                let status = if *bias > 1.1 {
                    "excellent"
                } else if *bias > 0.9 {
                    "good"
                } else if *bias > 0.7 {
                    "struggling"
                } else {
                    "avoid"
                };
                guidance.push_str(&format!("  - {}: {} (bias: {:.2})\n", model, status, bias));
            }
        }

        guidance
    }

    /// Track a tool execution outcome and update adaptive state.
    pub fn track_tool_outcome(&self, tool_name: &str, success: bool, _latency_ms: u64) {
        let mut state = match self.adaptive_state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let threshold = state
            .tool_confidence
            .entry(tool_name.to_string())
            .or_insert(0.7);

        // Adjust threshold based on success/failure
        if success {
            // Gradually lower threshold for reliable tools (more auto-execute)
            *threshold = (*threshold * 0.95).max(0.5);
        } else {
            // Raise threshold for unreliable tools (more confirmation)
            *threshold = (*threshold * 1.05).min(0.95);
        }

        info!(
            "Tool '{}' outcome: {} → confidence threshold adjusted to {:.2}",
            tool_name,
            if success { "success" } else { "failure" },
            *threshold
        );
    }

    /// Track a model's performance for a task type.
    pub fn track_model_performance(&self, model: &str, task_type: &str, success: bool) {
        let mut state = match self.adaptive_state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let key = format!("{}:{}", model, task_type);
        let bias = state.model_bias.entry(key).or_insert(1.0);

        // Exponential moving average
        let alpha = 0.3;
        let outcome = if success { 1.2 } else { 0.8 };
        *bias = (*bias * (1.0 - alpha)) + (outcome * alpha);

        info!(
            "Model '{}' on '{}' task: {} → bias updated to {:.2}",
            model,
            task_type,
            if success { "success" } else { "failure" },
            *bias
        );
    }

    /// Check if we should trigger self-analysis.
    pub fn should_trigger_analysis(&self) -> bool {
        let state = match self.adaptive_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };

        state.auto_analysis_enabled && state.sessions_since_analysis >= state.analysis_threshold
    }

    /// Increment session counter and return whether analysis should trigger.
    pub fn record_session(&self) -> bool {
        let mut state = match self.adaptive_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };

        state.sessions_since_analysis += 1;
        state.total_sessions += 1;

        let should_trigger = state.auto_analysis_enabled
            && state.sessions_since_analysis >= state.analysis_threshold;

        if should_trigger {
            state.sessions_since_analysis = 0;
        }

        should_trigger
    }

    /// Save adaptive state to memory store.
    pub fn save_state(&self) -> Result<()> {
        let state = self
            .adaptive_state
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock adaptive state"))?;

        let memory = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock memory"))?;

        let state_json =
            serde_json::to_string(&*state).context("Failed to serialize adaptive state")?;

        memory.save_analysis_result("adaptive_state", "current", &state_json)?;

        info!("Adaptive state saved");
        Ok(())
    }

    /// Load adaptive state from memory store.
    fn load_adaptive_state(memory: &MemoryStore) -> Option<AdaptiveState> {
        match memory.get_analysis_result("adaptive_state", "current") {
            Ok(Some(json)) => serde_json::from_str(&json).ok(),
            _ => None,
        }
    }

    /// Get the current adaptive state as a debug string.
    pub fn state_summary(&self) -> String {
        let state = match self.adaptive_state.lock() {
            Ok(s) => s,
            Err(_) => return "Adaptive state unavailable".to_string(),
        };

        let mut summary = format!(
            "Adaptive State (sessions: {}, since analysis: {})\n",
            state.total_sessions, state.sessions_since_analysis
        );

        summary.push_str("\nTool Confidence Thresholds:\n");
        for (tool, threshold) in &state.tool_confidence {
            let status = if *threshold <= 0.6 {
                "🟢 auto"
            } else if *threshold <= 0.8 {
                "🟡 confirm"
            } else {
                "🔒 manual"
            };
            summary.push_str(&format!("  {}: {:.2} {}\n", tool, threshold, status));
        }

        if !state.model_bias.is_empty() {
            summary.push_str("\nModel Performance Bias:\n");
            for (model, bias) in &state.model_bias {
                summary.push_str(&format!("  {}: {:.2}\n", model, bias));
            }
        }

        summary
    }
}

/// Convenience function to create an evolution engine from config.
pub fn create_engine(config: &Config) -> Result<EvolutionEngine> {
    EvolutionEngine::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_adaptive_state_default() {
        let state = AdaptiveState::default();
        assert_eq!(state.tool_confidence.get("fs").copied().unwrap(), 0.7);
        assert_eq!(state.analysis_threshold, 20);
        assert!(state.auto_analysis_enabled);
    }

    #[test]
    fn test_tool_confidence_adjustment() {
        let config = Config::default();
        let engine = EvolutionEngine::new(&config).unwrap();

        // Track multiple successes
        for _ in 0..10 {
            engine.track_tool_outcome("fs", true, 100);
        }

        let state = engine.adaptive_state.lock().unwrap();
        let threshold = state.tool_confidence.get("fs").copied().unwrap();
        assert!(
            threshold < 0.7,
            "Success should lower threshold, got {}",
            threshold
        );
    }

    #[test]
    fn test_tool_confidence_failure_adjustment() {
        let config = Config::default();
        let engine = EvolutionEngine::new(&config).unwrap();

        // Track multiple failures
        for _ in 0..10 {
            engine.track_tool_outcome("terminal", false, 100);
        }

        let state = engine.adaptive_state.lock().unwrap();
        let threshold = state.tool_confidence.get("terminal").copied().unwrap();
        assert!(
            threshold > 0.8,
            "Failure should raise threshold, got {}",
            threshold
        );
    }

    #[test]
    fn test_model_bias_tracking() {
        let config = Config::default();
        let engine = EvolutionEngine::new(&config).unwrap();

        engine.track_model_performance("kimi-k2.6", "code", true);
        engine.track_model_performance("kimi-k2.6", "code", true);
        engine.track_model_performance("kimi-k2.6", "code", true);

        let state = engine.adaptive_state.lock().unwrap();
        let bias = state.model_bias.get("kimi-k2.6:code").copied().unwrap();
        assert!(bias > 1.0, "Success should increase bias, got {}", bias);
    }

    #[test]
    fn test_session_counter() {
        let config = Config::default();
        let engine = EvolutionEngine::new(&config).unwrap();

        // Record 19 sessions — should not trigger
        for _ in 0..19 {
            assert!(!engine.record_session());
        }

        // 20th session — should trigger
        assert!(engine.record_session());

        // Next one should not trigger immediately
        assert!(!engine.record_session());
    }

    #[test]
    fn test_state_summary() {
        let config = Config::default();
        let engine = EvolutionEngine::new(&config).unwrap();

        let summary = engine.state_summary();
        assert!(summary.contains("Adaptive State"));
        assert!(summary.contains("Tool Confidence"));
    }
}
