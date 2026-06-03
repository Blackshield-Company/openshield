use crate::config::{Config, ModelConfig};
use crate::memory::{MemoryStore, Session, ToolCall};
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Weights for the scoring system. These can be tuned based on operational priorities.
const WEIGHT_SUCCESS_RATE: f64 = 0.40;
const WEIGHT_CAPABILITY_MATCH: f64 = 0.35;
const WEIGHT_COST_EFFICIENCY: f64 = 0.25;

/// Minimum number of historical samples before we fully trust success-rate data.
const MIN_SAMPLES_FOR_FULL_TRUST: usize = 5;

/// Default success rate assumed for models with no historical data.
const DEFAULT_SUCCESS_RATE: f64 = 0.75;

/// Fallback provider name when no provider is found.
const FALLBACK_PROVIDER: &str = "local";

/// Estimated tokens per word for context-length heuristics.
const TOKENS_PER_WORD: f64 = 1.35;

/// Maximum fraction of a model's context window that a task should occupy.
const CONTEXT_SAFETY_MARGIN: f64 = 0.85;

#[derive(Debug, Clone)]
pub struct RoutingDecision {
    #[allow(dead_code)]
    pub task_type: String,
    pub model: String,
    #[allow(dead_code)]
    pub provider: String,
    #[allow(dead_code)]
    pub reason: String,
}

/// Score breakdown for transparency in routing decisions.
#[derive(Debug, Clone)]
pub struct ScoreBreakdown {
    pub model: String,
    #[allow(dead_code)]
    pub provider: String,
    pub success_rate: f64,
    pub capability_match: f64,
    pub cost_efficiency: f64,
    pub total_score: f64,
    pub context_ok: bool,
    pub within_budget: bool,
}

/// Provider health status. In a real system this would be updated by a health-check loop.
#[derive(Debug, Clone, Default)]
pub struct ProviderHealth {
    /// Provider name -> is healthy
    pub status: HashMap<String, bool>,
}

impl ProviderHealth {
    pub fn new() -> Self {
        Self {
            status: HashMap::new(),
        }
    }

    pub fn is_healthy(&self, provider: &str) -> bool {
        self.status.get(provider).copied().unwrap_or(true)
    }

    #[allow(dead_code)]
    pub fn mark_unhealthy(&mut self, provider: &str) {
        self.status.insert(provider.to_string(), false);
    }

    #[allow(dead_code)]
    pub fn mark_healthy(&mut self, provider: &str) {
        self.status.insert(provider.to_string(), true);
    }
}

/// Estimate the number of tokens a task description might require.
fn estimate_task_tokens(task_description: &str) -> usize {
    let word_count = task_description.split_whitespace().count();
    (word_count as f64 * TOKENS_PER_WORD).ceil() as usize
}

/// Estimate task complexity based on description heuristics.
fn estimate_task_complexity(task_description: &str) -> usize {
    let desc = task_description.to_lowercase();
    let base_tokens = estimate_task_tokens(task_description);

    // Complexity multipliers based on keywords
    let multiplier = if desc.contains("refactor entire") || desc.contains("rewrite whole") {
        4.0
    } else if desc.contains("architect") || desc.contains("design system") {
        3.5
    } else if desc.contains("debug") || desc.contains("fix") {
        2.0
    } else if desc.contains("test") || desc.contains("testing") {
        1.8
    } else if desc.contains("explain") || desc.contains("document") {
        1.5
    } else if desc.contains("analyze") {
        2.2
    } else {
        1.0
    };

    (base_tokens as f64 * multiplier).ceil() as usize
}

/// Check if a model's context length can accommodate the estimated task complexity.
fn model_can_handle_context(model: &ModelConfig, task_description: &str) -> bool {
    let estimated_tokens = estimate_task_complexity(task_description);
    let safe_limit = (model.context_length as f64 * CONTEXT_SAFETY_MARGIN) as usize;
    estimated_tokens <= safe_limit
}

/// Compute historical success rate for a given model and optional task type.
fn compute_success_rate(
    sessions: &[Session],
    tool_calls: &HashMap<String, Vec<ToolCall>>,
    model: &str,
    task_type: Option<&str>,
) -> f64 {
    let mut total_success = 0usize;
    let mut total_calls = 0usize;

    for session in sessions {
        if session.model != model {
            continue;
        }
        if let Some(tt) = task_type
            && session.task_type != tt {
                continue;
            }

        if let Some(calls) = tool_calls.get(&session.id) {
            let success_count = calls.iter().filter(|tc| tc.success).count();
            total_success += success_count;
            total_calls += calls.len();
        }
    }

    if total_calls == 0 {
        return DEFAULT_SUCCESS_RATE;
    }

    let rate = total_success as f64 / total_calls as f64;

    // If we have few samples, blend with the default to avoid overfitting
    if total_calls < MIN_SAMPLES_FOR_FULL_TRUST {
        let blend_factor = total_calls as f64 / MIN_SAMPLES_FOR_FULL_TRUST as f64;
        rate * blend_factor + DEFAULT_SUCCESS_RATE * (1.0 - blend_factor)
    } else {
        rate
    }
}

/// Compute cost efficiency score (lower cost = higher score).
/// Returns a normalized score between 0.0 and 1.0.
fn compute_cost_efficiency(model: &ModelConfig, all_models: &[(&ModelConfig, &str)]) -> f64 {
    let total_cost = model.cost_per_1k_input + model.cost_per_1k_output;

    if all_models.is_empty() {
        return 0.5;
    }

    let max_cost = all_models
        .iter()
        .map(|(m, _)| m.cost_per_1k_input + m.cost_per_1k_output)
        .fold(0.0, f64::max);

    let min_cost = all_models
        .iter()
        .map(|(m, _)| m.cost_per_1k_input + m.cost_per_1k_output)
        .fold(f64::MAX, f64::min);

    if (max_cost - min_cost).abs() < f64::EPSILON {
        return 1.0;
    }

    // Invert so lower cost = higher score
    1.0 - ((total_cost - min_cost) / (max_cost - min_cost))
}

/// Compute capability match score between 0.0 and 1.0.
fn compute_capability_match(model: &ModelConfig, task_type: &str) -> f64 {
    let task_lower = task_type.to_lowercase();
    let matches = model
        .capabilities
        .iter()
        .any(|c| c.to_lowercase() == task_lower);

    if matches {
        1.0
    } else {
        // Partial match: check if any capability contains the task type or vice versa
        let partial = model.capabilities.iter().any(|c| {
            c.to_lowercase().contains(&task_lower) || task_lower.contains(&c.to_lowercase())
        });
        if partial { 0.5 } else { 0.0 }
    }
}

/// Build a map of session_id -> tool_calls from the memory store.
fn build_tool_calls_map(
    memory: &MemoryStore,
    sessions: &[Session],
) -> Result<HashMap<String, Vec<ToolCall>>> {
    let mut map = HashMap::new();
    for session in sessions {
        let calls = memory
            .search_tool_calls_by_session(&session.id)
            .with_context(|| format!("Failed to load tool calls for session {}", session.id))?;
        map.insert(session.id.clone(), calls);
    }
    Ok(map)
}

/// Core routing function that selects the best model for a task.
///
/// This is the primary entry point for automatic model selection. It:
/// 1. Classifies the task type
/// 2. Loads historical performance data
/// 3. Scores all available models
/// 4. Enforces cost limits and context requirements
/// 5. Returns a `RoutingDecision` with detailed reasoning
pub async fn route_task(config: &Config, task_description: &str) -> Result<RoutingDecision> {
    let task_type = classify_task(task_description);

    let memory = MemoryStore::new(&config.memory_db_path)
        .context("Failed to open memory store for routing")?;
    let sessions = memory
        .get_recent_sessions(100)
        .context("Failed to load recent sessions")?;
    let tool_calls_map = build_tool_calls_map(&memory, &sessions)?;

    let health = ProviderHealth::new(); // Default: all healthy

    let decision = find_best_model_with_data(
        config,
        &task_type,
        task_description,
        &sessions,
        &tool_calls_map,
        &health,
    )?;

    Ok(decision)
}

pub fn find_best_model_for_task(config: &Config, task_type: &str) -> String {
    if !config.auto_route {
        return config.default_model.clone();
    }

    let memory_result = MemoryStore::new(&config.memory_db_path);
    let (sessions, tool_calls_map) = match memory_result {
        Ok(memory) => match memory.get_recent_sessions(100) {
            Ok(sessions) => match build_tool_calls_map(&memory, &sessions) {
                Ok(map) => (sessions, map),
                Err(_) => (Vec::new(), HashMap::new()),
            },
            Err(_) => (Vec::new(), HashMap::new()),
        },
        Err(_) => (Vec::new(), HashMap::new()),
    };

    let health = ProviderHealth::new();

    let result =
        find_best_model_with_data(config, task_type, "", &sessions, &tool_calls_map, &health);

    match result {
        Ok(decision) => decision.model,
        Err(_) => config.default_model.clone(),
    }
}

/// Internal function that performs the actual scoring and selection.
fn find_best_model_with_data(
    config: &Config,
    task_type: &str,
    task_description: &str,
    sessions: &[Session],
    tool_calls_map: &HashMap<String, Vec<ToolCall>>,
    health: &ProviderHealth,
) -> Result<RoutingDecision> {
    if !config.auto_route {
        let provider = find_provider_for_model(config, &config.default_model);
        return Ok(RoutingDecision {
            task_type: task_type.to_string(),
            model: config.default_model.clone(),
            provider,
            reason: "Auto-route disabled; using default model".to_string(),
        });
    }

    // Collect all models across all providers
    let mut all_models: Vec<(&ModelConfig, &str)> = Vec::new();
    for (provider_name, provider) in &config.providers {
        for model in &provider.models {
            all_models.push((model, provider_name.as_str()));
        }
    }

    if all_models.is_empty() {
        anyhow::bail!("No models configured");
    }

    // Score each model
    let mut scored_models: Vec<ScoreBreakdown> = Vec::new();
    for (model, provider_name) in &all_models {
        let success_rate =
            compute_success_rate(sessions, tool_calls_map, &model.name, Some(task_type));
        let capability_match = compute_capability_match(model, task_type);
        let cost_efficiency = compute_cost_efficiency(model, &all_models);
        let context_ok = model_can_handle_context(model, task_description);
        let within_budget = is_within_budget(model, config.cost_limit_usd);
        let provider_healthy = health.is_healthy(provider_name);

        // Penalize models that don't meet hard constraints
        let constraint_penalty = if !context_ok {
            0.0 // Hard fail: context too small
        } else if !within_budget {
            0.1 // Severe penalty: over budget
        } else if !provider_healthy {
            0.2 // Significant penalty: unhealthy provider
        } else {
            1.0
        };

        let total_score = (success_rate * WEIGHT_SUCCESS_RATE
            + capability_match * WEIGHT_CAPABILITY_MATCH
            + cost_efficiency * WEIGHT_COST_EFFICIENCY)
            * constraint_penalty;

        scored_models.push(ScoreBreakdown {
            model: model.name.clone(),
            provider: provider_name.to_string(),
            success_rate,
            capability_match,
            cost_efficiency,
            total_score,
            context_ok,
            within_budget,
        });
    }

    // Sort by total score descending
    scored_models.sort_by(|a, b| {
        b.total_score
            .partial_cmp(&a.total_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Pick the best model that meets all hard constraints
    let best = scored_models
        .iter()
        .find(|s| s.context_ok && health.is_healthy(&s.provider))
        .or_else(|| scored_models.first())
        .context("No suitable model found")?;

    let mut reasons = Vec::new();
    reasons.push(format!("score={:.3}", best.total_score));
    reasons.push(format!("success_rate={:.1}%", best.success_rate * 100.0));
    reasons.push(format!(
        "capability_match={:.1}%",
        best.capability_match * 100.0
    ));
    reasons.push(format!(
        "cost_efficiency={:.1}%",
        best.cost_efficiency * 100.0
    ));

    if !best.within_budget {
        reasons.push("WARNING: exceeds cost limit".to_string());
    }
    if !best.context_ok {
        reasons.push("WARNING: context may be insufficient".to_string());
    }

    let reason = reasons.join(", ");

    Ok(RoutingDecision {
        task_type: task_type.to_string(),
        model: best.model.clone(),
        provider: best.provider.clone(),
        reason,
    })
}

/// Check if a model's cost is within the configured limit.
/// We estimate a typical session cost: 2k input + 1k output tokens.
fn is_within_budget(model: &ModelConfig, cost_limit_usd: f64) -> bool {
    let estimated_input_tokens = 2000.0;
    let estimated_output_tokens = 1000.0;
    let estimated_cost = (estimated_input_tokens / 1000.0) * model.cost_per_1k_input
        + (estimated_output_tokens / 1000.0) * model.cost_per_1k_output;
    estimated_cost <= cost_limit_usd
}

/// Performance metrics for routing decisions.
#[derive(Debug, Clone)]
pub struct RouterStats {
    pub total_routes: usize,
    pub avg_success_rate: f64,
    pub top_model: String,
    pub top_model_usage: usize,
}

/// Get router performance statistics.
pub async fn get_router_stats(config: &Config) -> Result<RouterStats> {
    let memory = MemoryStore::new(&config.memory_db_path).context("Failed to open memory store")?;
    let recent_sessions = memory
        .get_recent_sessions(100)
        .context("Failed to load recent sessions")?;

    let tool_calls_map = build_tool_calls_map(&memory, &recent_sessions)?;

    let mut model_usage: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut total_success = 0usize;
    let mut total_calls = 0usize;

    for session in &recent_sessions {
        *model_usage.entry(session.model.clone()).or_insert(0) += 1;
        if let Some(calls) = tool_calls_map.get(&session.id) {
            total_success += calls.iter().filter(|tc| tc.success).count();
            total_calls += calls.len();
        }
    }

    let (top_model, top_model_usage) = model_usage
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .unwrap_or_else(|| (config.default_model.clone(), 0));

    let avg_success_rate = if total_calls > 0 {
        (total_success as f64 / total_calls as f64) * 100.0
    } else {
        0.0
    };

    Ok(RouterStats {
        total_routes: recent_sessions.len(),
        avg_success_rate,
        top_model,
        top_model_usage,
    })
}

/// Display current routing decisions and model performance analytics.
pub async fn show_decisions(config: &Config) -> Result<()> {
    let memory = MemoryStore::new(&config.memory_db_path).context("Failed to open memory store")?;
    let recent_sessions = memory
        .get_recent_sessions(50)
        .context("Failed to load recent sessions")?;

    println!("🦈 Routing Decisions");
    println!(
        "Auto-route: {}",
        if config.auto_route {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("Default model: {}", config.default_model);
    println!("Cost limit: ${:.2}", config.cost_limit_usd);
    println!();

    // Build tool calls map once
    let tool_calls_map = build_tool_calls_map(&memory, &recent_sessions)?;

    if recent_sessions.is_empty() {
        println!("No session history yet. Using capability-based routing.");
        println!();
        show_capability_routing(config);
    } else {
        println!(
            "Recent session analysis (last {} sessions):",
            recent_sessions.len()
        );
        println!();

        let mut model_success: HashMap<String, (usize, usize)> = HashMap::new();
        let mut task_model: HashMap<String, HashMap<String, (usize, usize)>> = HashMap::new();

        for session in &recent_sessions {
            let tool_calls = tool_calls_map.get(&session.id).cloned().unwrap_or_default();
            let success_count = tool_calls.iter().filter(|tc| tc.success).count();
            let total_count = tool_calls.len();

            let entry = model_success.entry(session.model.clone()).or_insert((0, 0));
            entry.0 += success_count;
            entry.1 += total_count;

            let task_entry = task_model.entry(session.task_type.clone()).or_default();
            let model_entry = task_entry.entry(session.model.clone()).or_insert((0, 0));
            model_entry.0 += success_count;
            model_entry.1 += total_count;
        }

        println!("Model Performance:");
        println!(
            "{:<20} | {:>8} | {:>8} | {:>6}",
            "Model", "Success", "Total", "Rate"
        );
        println!("{}", "-".repeat(50));
        for (model, (success, total)) in &model_success {
            let rate = if *total > 0 {
                (*success as f64 / *total as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "{:<20} | {:>8} | {:>8} | {:>5.1}%",
                model, success, total, rate
            );
        }
        println!();

        println!("Task-Type Routing (with historical data):");
        println!(
            "{:<15} | {:<20} | {:>6} | Reason",
            "Task Type", "Best Model", "Rate"
        );
        println!("{}", "-".repeat(70));
        for (task_type, models) in &task_model {
            let best = models
                .iter()
                .max_by_key(|(_, (s, t))| (*s * 100).checked_div(*t).unwrap_or(0))
                .map(|(m, _)| m.clone())
                .unwrap_or_else(|| config.default_model.clone());

            let (success, total) = models.get(&best).unwrap_or(&(0, 0));
            let rate = if *total > 0 {
                (*success as f64 / *total as f64) * 100.0
            } else {
                0.0
            };

            println!(
                "{:<15} | {:<20} | {:>5.1}% | Historical best",
                task_type, best, rate
            );
        }
        println!();
    }

    println!("Available Models (with scoring context):");
    println!(
        "{:<20} | {:<10} | {:>10} | {:>10} | {:<30}",
        "Model", "Provider", "Ctx Len", "Cost/1K", "Capabilities"
    );
    println!("{}", "-".repeat(100));
    for (provider_name, provider) in &config.providers {
        for model in &provider.models {
            let cost = model.cost_per_1k_input + model.cost_per_1k_output;
            println!(
                "{:<20} | {:<10} | {:>10} | ${:>9.4} | {}",
                model.name,
                provider_name,
                model.context_length,
                cost,
                model.capabilities.join(", ")
            );
        }
    }
    println!();

    // Show a sample routing decision for each task type
    println!("Sample Routing Decisions:");
    println!(
        "{:<15} | {:<20} | {:<15} | Reason",
        "Task Type", "Model", "Provider"
    );
    println!("{}", "-".repeat(90));
    let sample_tasks = vec![
        ("code", "Refactor the authentication module"),
        ("analysis", "Analyze system architecture"),
        ("chat", "Explain how async works in Rust"),
    ];
    for (task_type, _description) in sample_tasks {
        let decision = find_best_model_for_task(config, task_type);
        let provider = find_provider_for_model(config, &decision);
        println!(
            "{:<15} | {:<20} | {:<15} | Capability match",
            task_type, decision, provider
        );
    }

    Ok(())
}

fn show_capability_routing(config: &Config) {
    println!("Capability-Based Routing Rules:");
    println!(
        "{:<15} | {:<20} | Logic",
        "Task Type", "Preferred Model"
    );
    println!("{}", "-".repeat(70));

    let rules = vec![
        ("code", "Find model with 'code' capability, lowest cost"),
        ("chat", "Find model with 'chat' capability, lowest cost"),
        ("analysis", "Find model with 'analysis' capability"),
        ("default", "Use default model from config"),
    ];

    for (task, logic) in &rules {
        let preferred = find_best_model_for_task(config, task);
        println!("{:<15} | {:<20} | {}", task, preferred, logic);
    }
    println!();
}

fn find_provider_for_model(config: &Config, model_name: &str) -> String {
    for (provider_name, provider) in &config.providers {
        if provider.models.iter().any(|m| m.name == model_name) {
            return provider_name.clone();
        }
    }
    FALLBACK_PROVIDER.to_string()
}

fn classify_task(description: &str) -> String {
    let desc = description.to_lowercase();

    if desc.contains("refactor") || desc.contains("rewrite") || desc.contains("restructure")
        || desc.contains("debug") || desc.contains("fix") || desc.contains("error")
        || desc.contains("test") || desc.contains("testing") {
        "code".to_string()
    } else if desc.contains("architect") || desc.contains("design") || desc.contains("structure")
        || desc.contains("explain") || desc.contains("document") || desc.contains("analyze") {
        "analysis".to_string()
    } else {
        "chat".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ModelConfig, ProviderConfig};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn create_test_config() -> Config {
        let mut providers = HashMap::new();

        providers.insert(
            "kimi".to_string(),
            ProviderConfig {
                base_url: "https://api.kimi.com/coding/v1".to_string(),
                api_key: "test-key".to_string(),
                models: vec![ModelConfig {
                    name: "kimi-k2.6".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.01,
                    cost_per_1k_output: 0.02,
                    capabilities: vec![
                        "code".to_string(),
                        "chat".to_string(),
                        "analysis".to_string(),
                    ],
                }],
                kind: crate::config::ProviderKind::OpenAiCompatible,
                headers: std::collections::HashMap::new(),
                env_file: None,
            },
        );

        providers.insert(
            "opencode".to_string(),
            ProviderConfig {
                base_url: "https://api.opencode.ai/v1".to_string(),
                api_key: "test-key".to_string(),
                models: vec![ModelConfig {
                    name: "deepseek-v4-flash-free".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string()],
                }],
                kind: crate::config::ProviderKind::OpenAiCompatible,
                headers: std::collections::HashMap::new(),
                env_file: None,
            },
        );

        providers.insert(
            "budget".to_string(),
            ProviderConfig {
                base_url: "https://api.budget.ai/v1".to_string(),
                api_key: "test-key".to_string(),
                models: vec![ModelConfig {
                    name: "budget-model".to_string(),
                    context_length: 32000,
                    cost_per_1k_input: 0.001,
                    cost_per_1k_output: 0.002,
                    capabilities: vec!["chat".to_string()],
                }],
                kind: crate::config::ProviderKind::OpenAiCompatible,
                headers: std::collections::HashMap::new(),
                env_file: None,
            },
        );

        Config {
            version: crate::VERSION.to_string(),
            default_model: "kimi-k2.6".to_string(),
            providers,
            memory_db_path: PathBuf::from("/tmp/test_openshark_router_memory_new.db"),
            tools_enabled: vec!["fs".to_string(), "terminal".to_string()],
            auto_route: true,
            cost_limit_usd: 10.0,
            agent: crate::config::AgentIdentity::default(),
            gateway: crate::gateway::GatewayConfig::default(),
            user_name: "user".to_string(),
            theme: "synthwave84".to_string(),
            filesystem: crate::config::FilesystemConfig::default(),
            swarm: crate::swarm::SwarmConfig::default(),
            context_compression: crate::memory::compression::ContextCompressionConfig::default(),
            keybindings: crate::config::KeybindingsConfig::default(),
            auto_commit: false,
            auto_commit_model: None,
            weak_model: None,
            architect_model: None,
            editor_model: None,
            auto_run_tests: false,
            test_command: None,
            effort_level: "medium".to_string(),
        }
    }

    fn create_test_config_with_small_context() -> Config {
        let mut providers = HashMap::new();

        providers.insert(
            "tiny".to_string(),
            ProviderConfig {
                base_url: "https://api.tiny.ai/v1".to_string(),
                api_key: "test-key".to_string(),
                models: vec![ModelConfig {
                    name: "tiny-model".to_string(),
                    context_length: 100,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string()],
                }],
                kind: crate::config::ProviderKind::OpenAiCompatible,
                headers: std::collections::HashMap::new(),
                env_file: None,
            },
        );

        Config {
            version: crate::VERSION.to_string(),
            default_model: "tiny-model".to_string(),
            providers,
            memory_db_path: PathBuf::from("/tmp/test_openshark_router_memory2_new.db"),
            tools_enabled: vec![],
            auto_route: true,
            cost_limit_usd: 10.0,
            agent: crate::config::AgentIdentity::default(),
            gateway: crate::gateway::GatewayConfig::default(),
            user_name: "user".to_string(),
            theme: "synthwave84".to_string(),
            filesystem: crate::config::FilesystemConfig::default(),
            swarm: crate::swarm::SwarmConfig::default(),
            context_compression: crate::memory::compression::ContextCompressionConfig::default(),
            keybindings: crate::config::KeybindingsConfig::default(),
            auto_commit: false,
            auto_commit_model: None,
            weak_model: None,
            architect_model: None,
            editor_model: None,
            auto_run_tests: false,
            test_command: None,
            effort_level: "medium".to_string(),
        }
    }

    #[test]
    fn test_classify_task_code() {
        assert_eq!(classify_task("Refactor the auth module"), "code");
        assert_eq!(classify_task("Debug this error"), "code");
        assert_eq!(classify_task("Write tests for the API"), "code");
    }

    #[test]
    fn test_classify_task_analysis() {
        assert_eq!(classify_task("Analyze system architecture"), "analysis");
        assert_eq!(classify_task("Document the API endpoints"), "analysis");
    }

    #[test]
    fn test_classify_task_chat() {
        assert_eq!(classify_task("Hello, how are you?"), "chat");
        assert_eq!(classify_task("What is Rust?"), "chat");
    }

    #[test]
    fn test_find_provider_for_model() {
        let config = create_test_config();
        assert_eq!(find_provider_for_model(&config, "kimi-k2.6"), "kimi");
        assert_eq!(
            find_provider_for_model(&config, "deepseek-v4-flash-free"),
            "opencode"
        );
        assert_eq!(find_provider_for_model(&config, "nonexistent"), "local");
    }

    #[test]
    fn test_compute_capability_match_exact() {
        let model = ModelConfig {
            name: "test".to_string(),
            context_length: 1000,
            cost_per_1k_input: 0.0,
            cost_per_1k_output: 0.0,
            capabilities: vec!["code".to_string(), "chat".to_string()],
        };
        assert_eq!(compute_capability_match(&model, "code"), 1.0);
        assert_eq!(compute_capability_match(&model, "chat"), 1.0);
    }

    #[test]
    fn test_compute_capability_match_none() {
        let model = ModelConfig {
            name: "test".to_string(),
            context_length: 1000,
            cost_per_1k_input: 0.0,
            cost_per_1k_output: 0.0,
            capabilities: vec!["chat".to_string()],
        };
        assert_eq!(compute_capability_match(&model, "code"), 0.0);
    }

    #[test]
    fn test_compute_capability_match_partial() {
        let model = ModelConfig {
            name: "test".to_string(),
            context_length: 1000,
            cost_per_1k_input: 0.0,
            cost_per_1k_output: 0.0,
            capabilities: vec!["code-review".to_string()],
        };
        assert_eq!(compute_capability_match(&model, "code"), 0.5);
    }

    #[test]
    fn test_estimate_task_tokens() {
        let tokens = estimate_task_tokens("Hello world this is a test");
        assert!(tokens > 0);
        assert_eq!(tokens, 9);
    }

    #[test]
    fn test_estimate_task_complexity() {
        let simple = estimate_task_complexity("Hello world");
        let complex = estimate_task_complexity("Refactor entire codebase architecture");
        assert!(complex > simple);
    }

    #[test]
    fn test_model_can_handle_context() {
        let model = ModelConfig {
            name: "test".to_string(),
            context_length: 1000,
            cost_per_1k_input: 0.0,
            cost_per_1k_output: 0.0,
            capabilities: vec![],
        };
        assert!(model_can_handle_context(&model, "Short task"));
        // A very long task description should still fit in 1000 tokens with margin
        let long_desc = "word ".repeat(500);
        assert!(model_can_handle_context(&model, &long_desc));
    }

    #[test]
    fn test_model_cannot_handle_context() {
        let config = create_test_config_with_small_context();
        let model = &config.providers.get("tiny").unwrap().models[0];
        let long_desc = "word ".repeat(200);
        assert!(!model_can_handle_context(model, &long_desc));
    }

    #[test]
    fn test_compute_cost_efficiency() {
        let model_free = ModelConfig {
            name: "free".to_string(),
            context_length: 1000,
            cost_per_1k_input: 0.0,
            cost_per_1k_output: 0.0,
            capabilities: vec![],
        };
        let model_expensive = ModelConfig {
            name: "expensive".to_string(),
            context_length: 1000,
            cost_per_1k_input: 0.1,
            cost_per_1k_output: 0.2,
            capabilities: vec![],
        };
        let all_models: Vec<(&ModelConfig, &str)> =
            vec![(&model_free, "p1"), (&model_expensive, "p2")];

        let free_score = compute_cost_efficiency(&model_free, &all_models);
        let expensive_score = compute_cost_efficiency(&model_expensive, &all_models);

        assert!(free_score > expensive_score);
        assert_eq!(free_score, 1.0);
    }

    #[test]
    fn test_is_within_budget() {
        let cheap_model = ModelConfig {
            name: "cheap".to_string(),
            context_length: 1000,
            cost_per_1k_input: 0.001,
            cost_per_1k_output: 0.002,
            capabilities: vec![],
        };
        assert!(is_within_budget(&cheap_model, 10.0));

        let expensive_model = ModelConfig {
            name: "expensive".to_string(),
            context_length: 1000,
            cost_per_1k_input: 100.0,
            cost_per_1k_output: 200.0,
            capabilities: vec![],
        };
        assert!(!is_within_budget(&expensive_model, 10.0));
    }

    #[test]
    fn test_compute_success_rate_no_data() {
        let sessions: Vec<Session> = Vec::new();
        let tool_calls: HashMap<String, Vec<ToolCall>> = HashMap::new();
        let rate = compute_success_rate(&sessions, &tool_calls, "any-model", None);
        assert_eq!(rate, DEFAULT_SUCCESS_RATE);
    }

    #[test]
    fn test_compute_success_rate_full_trust_router() {
        let sessions = vec![Session {
            id: "s1".to_string(),
            started_at: chrono::Utc::now(),
            model: "model-a".to_string(),
            task_type: "code".to_string(),
            project_path: None,
            archived: false,
        }];
        let mut tool_calls: HashMap<String, Vec<ToolCall>> = HashMap::new();
        // Need 5+ samples for full trust
        let mut calls = Vec::new();
        for i in 0..5 {
            calls.push(ToolCall {
                id: format!("tc{}", i),
                session_id: "s1".to_string(),
                tool_name: "test".to_string(),
                args: "{}".to_string(),
                result: "ok".to_string(),
                success: true,
                created_at: chrono::Utc::now(),
            });
        }
        tool_calls.insert("s1".to_string(), calls);
        let rate = compute_success_rate(&sessions, &tool_calls, "model-a", None);
        assert!((rate - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_provider_health() {
        let mut health = ProviderHealth::new();
        assert!(health.is_healthy("any"));
        health.mark_unhealthy("kimi");
        assert!(!health.is_healthy("kimi"));
        assert!(health.is_healthy("opencode"));
        health.mark_healthy("kimi");
        assert!(health.is_healthy("kimi"));
    }

    #[test]
    fn test_find_best_model_for_task_auto_route_disabled() {
        let mut config = create_test_config();
        config.auto_route = false;
        let result = find_best_model_for_task(&config, "code");
        assert_eq!(result, config.default_model);
    }

    #[test]
    fn test_find_best_model_for_task_with_capabilities() {
        let config = create_test_config();
        let result = find_best_model_for_task(&config, "code");
        // deepseek-v4-flash-free is free (cost=0) and has 'code' capability
        assert_eq!(result, "deepseek-v4-flash-free");
    }

    #[test]
    fn test_find_best_model_for_task_analysis() {
        let config = create_test_config();
        let result = find_best_model_for_task(&config, "analysis");
        // Only kimi-k2.6 has 'analysis' capability
        assert_eq!(result, "kimi-k2.6");
    }

    #[test]
    fn test_find_best_model_with_unhealthy_provider() {
        let config = create_test_config();
        let sessions: Vec<Session> = Vec::new();
        let tool_calls: HashMap<String, Vec<ToolCall>> = HashMap::new();
        let mut health = ProviderHealth::new();
        health.mark_unhealthy("opencode");

        let decision =
            find_best_model_with_data(&config, "code", "", &sessions, &tool_calls, &health)
                .unwrap();

        // opencode is unhealthy, so should fall back to another provider
        assert_ne!(decision.provider, "opencode");
    }

    #[test]
    fn test_find_best_model_with_context_constraint() {
        let config = create_test_config_with_small_context();
        let sessions: Vec<Session> = Vec::new();
        let tool_calls: HashMap<String, Vec<ToolCall>> = HashMap::new();
        let health = ProviderHealth::new();

        let long_desc = "word ".repeat(200);
        let decision =
            find_best_model_with_data(&config, "code", &long_desc, &sessions, &tool_calls, &health)
                .unwrap();

        // tiny-model has only 100 context length, so it should be flagged
        assert_eq!(decision.model, "tiny-model"); // Only model available
        assert!(decision.reason.contains("WARNING") || decision.reason.contains("context"));
    }

    #[tokio::test]
    async fn test_route_task_chat() {
        let mut config = create_test_config();
        config.memory_db_path = std::path::PathBuf::from(format!(
            "/tmp/openshark_router_test_chat_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&config.memory_db_path);
        let decision = route_task(&config, "Hello, how is Rust?").await.unwrap();
        assert_eq!(decision.task_type, "chat");
        assert!(!decision.model.is_empty());
    }

    #[tokio::test]
    async fn test_route_task_analysis() {
        let mut config = create_test_config();
        config.memory_db_path = std::path::PathBuf::from(format!(
            "/tmp/openshark_router_test_analysis_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&config.memory_db_path);
        let decision = route_task(&config, "Analyze the system architecture")
            .await
            .unwrap();
        assert_eq!(decision.task_type, "analysis");
        assert!(!decision.model.is_empty());
    }

    #[test]
    fn test_routing_decision_debug() {
        let decision = RoutingDecision {
            task_type: "code".to_string(),
            model: "test-model".to_string(),
            provider: "test-provider".to_string(),
            reason: "Test reason".to_string(),
        };
        let debug = format!("{:?}", decision);
        assert!(debug.contains("test-model"));
        assert!(debug.contains("Test reason"));
    }

    #[test]
    fn test_score_breakdown_sorting() {
        let mut scores = vec![
            ScoreBreakdown {
                model: "a".to_string(),
                provider: "p".to_string(),
                success_rate: 0.9,
                capability_match: 1.0,
                cost_efficiency: 0.8,
                total_score: 0.5,
                context_ok: true,
                within_budget: true,
            },
            ScoreBreakdown {
                model: "b".to_string(),
                provider: "p".to_string(),
                success_rate: 0.5,
                capability_match: 0.5,
                cost_efficiency: 0.5,
                total_score: 0.9,
                context_ok: true,
                within_budget: true,
            },
        ];
        scores.sort_by(|a, b| {
            b.total_score
                .partial_cmp(&a.total_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        assert_eq!(scores[0].model, "b");
        assert_eq!(scores[1].model, "a");
    }

    #[test]
    fn test_no_models_configured() {
        let mut config = create_test_config();
        config.providers.clear();
        let sessions: Vec<Session> = Vec::new();
        let tool_calls: HashMap<String, Vec<ToolCall>> = HashMap::new();
        let health = ProviderHealth::new();

        let result =
            find_best_model_with_data(&config, "code", "", &sessions, &tool_calls, &health);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_best_model_respects_cost_limit() {
        let mut config = create_test_config();
        config.cost_limit_usd = 0.001;
        let sessions: Vec<Session> = Vec::new();
        let tool_calls: HashMap<String, Vec<ToolCall>> = HashMap::new();
        let health = ProviderHealth::new();

        let decision =
            find_best_model_with_data(&config, "code", "", &sessions, &tool_calls, &health)
                .unwrap();

        assert_eq!(decision.model, "deepseek-v4-flash-free");
    }

    #[test]
    fn test_estimate_task_complexity_refactor() {
        let simple = estimate_task_complexity("Hello world");
        let refactor = estimate_task_complexity("Refactor entire codebase");
        let debug = estimate_task_complexity("Debug this error");
        let test = estimate_task_complexity("Write tests for API");

        assert!(refactor > simple);
        assert!(debug > simple);
        assert!(test > simple);
    }

    #[test]
    fn test_find_best_model_with_data_no_auto_route() {
        let mut config = create_test_config();
        config.auto_route = false;
        let sessions: Vec<Session> = Vec::new();
        let tool_calls: HashMap<String, Vec<ToolCall>> = HashMap::new();
        let health = ProviderHealth::new();

        let decision = find_best_model_with_data(
            &config,
            "code",
            "test task",
            &sessions,
            &tool_calls,
            &health,
        )
        .unwrap();

        assert_eq!(decision.model, config.default_model);
        assert!(decision.reason.contains("Auto-route disabled"));
    }

    #[test]
    fn test_provider_health_mark_all() {
        let mut health = ProviderHealth::new();
        assert!(health.is_healthy("any"));

        health.mark_unhealthy("provider1");
        health.mark_unhealthy("provider2");
        assert!(!health.is_healthy("provider1"));
        assert!(!health.is_healthy("provider2"));
        assert!(health.is_healthy("provider3"));

        health.mark_healthy("provider1");
        assert!(health.is_healthy("provider1"));
        assert!(!health.is_healthy("provider2"));
    }

    #[test]
    fn test_routing_decision_fields() {
        let decision = RoutingDecision {
            task_type: "code".to_string(),
            model: "test-model".to_string(),
            provider: "test-provider".to_string(),
            reason: "Test reason".to_string(),
        };

        assert_eq!(decision.task_type, "code");
        assert_eq!(decision.model, "test-model");
        assert_eq!(decision.provider, "test-provider");
        assert_eq!(decision.reason, "Test reason");
    }

    #[test]
    fn test_score_breakdown_fields() {
        let score = ScoreBreakdown {
            model: "test".to_string(),
            provider: "p".to_string(),
            success_rate: 0.9,
            capability_match: 1.0,
            cost_efficiency: 0.8,
            total_score: 0.95,
            context_ok: true,
            within_budget: true,
        };

        assert!(score.context_ok);
        assert!(score.within_budget);
        assert!(score.total_score > 0.0);
    }

    #[tokio::test]
    async fn test_route_task_code() {
        let mut config = create_test_config();
        config.memory_db_path = std::path::PathBuf::from(format!(
            "/tmp/openshark_router_test_code_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&config.memory_db_path);
        let decision = route_task(&config, "Refactor the auth module")
            .await
            .unwrap();
        assert_eq!(decision.task_type, "code");
        assert!(!decision.model.is_empty());
    }
}
