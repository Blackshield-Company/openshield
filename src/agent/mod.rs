use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::io;
use uuid::Uuid;

use crate::config::Config;
use crate::memory::{MemoryStore, Message as MemoryMessage, ToolCall};
use crate::providers::{ChatRequest, Message, Provider};
use crate::router::{route_task, RoutingDecision};
use crate::tools::{find_tool, get_tools};

pub mod soul;

pub const MAX_ITERATIONS: usize = 84;

/// Configuration for the agent.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Maximum iterations allowed per task.
    pub max_iterations: usize,
    /// Whether to require user approval before executing a plan.
    pub require_approval: bool,
    /// Default model to use when routing fails.
    pub default_model: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: MAX_ITERATIONS,
            require_approval: true,
            default_model: "kimi-k2.6".to_string(),
        }
    }
}

/// A single step in an execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Name of the tool to execute.
    pub tool_name: String,
    /// Arguments for the tool.
    pub args: String,
    /// Expected result description.
    pub expected_result: String,
    /// Criteria for verifying the step succeeded.
    pub verification_criteria: String,
}

/// An ordered list of plan steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// The steps to execute in order.
    pub steps: Vec<PlanStep>,
    /// Description of the overall plan.
    pub description: String,
}

/// Result of executing a single plan step.
#[derive(Debug, Clone)]
pub struct StepResult {
    /// The step that was executed.
    pub step: PlanStep,
    /// Raw output from the tool.
    pub output: String,
    /// Whether verification passed.
    pub verified: bool,
    /// Number of iterations spent on this step (including retries).
    pub iterations: usize,
}

/// Final result of running a task.
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// Whether the task completed successfully.
    pub success: bool,
    /// Results for each step.
    pub step_results: Vec<StepResult>,
    /// Total iterations used.
    pub total_iterations: usize,
    /// Final message or summary.
    pub message: String,
}

/// The autonomous agent that plans, executes, verifies, and iterates.
pub struct Agent {
    config: AgentConfig,
    memory: MemoryStore,
    provider: Provider,
    session_id: String,
    security_engine: crate::security::SecurityEngine,
}

impl Agent {
    /// Create a new agent with the given configuration.
    pub fn new(config: AgentConfig, app_config: &Config) -> Result<Self> {
        let memory = MemoryStore::new(&app_config.memory_db_path)
            .context("Failed to initialize memory store")?;
        let session_id = Uuid::new_v4().to_string();

        let model_name = &config.default_model;
        let (provider_name, provider_config) = app_config
            .find_provider_for_model(model_name)
            .unwrap_or_else(|| {
                let (n, p) = app_config.providers.iter().next().unwrap_or_else(|| {
                    panic!("No providers configured in config");
                });
                (n.clone(), p.clone())
            });

        let provider = Provider::new(
            provider_name.clone(),
            provider_config.base_url.clone(),
            provider_config.api_key.clone(),
            provider_config.kind.clone(),
            provider_config.headers.clone(),
        );

        let security_engine = crate::security::SecurityEngine::new(
            crate::security::SecurityConfig::load().unwrap_or_default()
        )?;

        memory.create_session(&session_id, model_name, "agentic")?;

        Ok(Self {
            config,
            memory,
            provider,
            session_id,
            security_engine,
        })
    }

    /// Main entry point: run a high-level task autonomously.
    pub async fn run_task(&self, task: &str) -> Result<TaskResult> {
        self.save_agent_message("system", &format!("Starting agentic task: {}", task))?;

        let mut plan = self.generate_plan(task).await?;
        let mut step_results: Vec<StepResult> = Vec::new();
        let mut total_iterations: usize = 0;

        // User approval loop
        if self.config.require_approval {
            match prompt_plan_approval(&plan) {
                PlanApproval::Approve => {}
                PlanApproval::Edit => {
                    plan = prompt_edit_plan(&plan)?;
                }
                PlanApproval::Reject => {
                    return Ok(TaskResult {
                        success: false,
                        step_results: Vec::new(),
                        total_iterations: 0,
                        message: "Plan rejected by user.".to_string(),
                    });
                }
            }
        }

        for (step_idx, step) in plan.steps.iter().enumerate() {
            if total_iterations >= self.config.max_iterations {
                return Ok(TaskResult {
                    success: false,
                    step_results,
                    total_iterations,
                    message: format!(
                        "Reached maximum iterations ({}). Stopping.",
                        self.config.max_iterations
                    ),
                });
            }

            let step_result = self.execute_step_with_retry(step, &mut total_iterations).await?;
            step_results.push(step_result.clone());

            self.save_agent_message(
                "assistant",
                &format!(
                    "Step {}: tool={}, verified={}, iterations={}",
                    step_idx + 1,
                    step_result.step.tool_name,
                    step_result.verified,
                    step_result.iterations
                ),
            )?;

            if !step_result.verified {
                // Escalate: try to create a recovery plan for this step
                let recovery_plan = self.escalate(step, &step_result.output).await?;
                if !recovery_plan.steps.is_empty() {
                    for recovery_step in &recovery_plan.steps {
                        if total_iterations >= self.config.max_iterations {
                            return Ok(TaskResult {
                                success: false,
                                step_results,
                                total_iterations,
                                message: format!(
                                    "Reached maximum iterations ({}) during recovery.",
                                    self.config.max_iterations
                                ),
                            });
                        }
                        let recovery_result = self
                            .execute_step_with_retry(recovery_step, &mut total_iterations)
                            .await?;
                        step_results.push(recovery_result);
                    }
                }
            }
        }

        let all_verified = step_results.iter().all(|r| r.verified);
        let message = if all_verified {
            "Task completed successfully.".to_string()
        } else {
            "Task completed with some unverified steps.".to_string()
        };

        Ok(TaskResult {
            success: all_verified,
            step_results,
            total_iterations,
            message,
        })
    }

    /// Generate a plan by asking the model to break down the task.
    pub async fn generate_plan(&self, task: &str) -> Result<Plan> {
        let routing_decision = route_task(
            &self.infer_config(),
            task,
        )
        .await
        .unwrap_or_else(|_| RoutingDecision {
            task_type: "agentic".to_string(),
            model: self.config.default_model.clone(),
            provider: "local".to_string(),
            reason: "Fallback".to_string(),
        });

        let tools_description = get_tools()
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");

        let soul = crate::agent::soul::load_soul_from_config(&self.infer_config());
        let system_prompt = format!(
            "{}\n\nYou are an autonomous planning agent. Break down tasks into concrete, actionable steps. \
             Each step must use one of these tools:\n{}\n\
             CRITICAL: Every plan step must have a clear expected_result and verification_criteria. \
             Vague or one-line plans are unacceptable. Be thorough and specific. \
             Respond ONLY with a JSON object in this exact format:\n\
             {{\"description\": \"brief plan description\", \"steps\": [\n\
             {{\"tool_name\": \"tool_name\", \"args\": \"arguments\", \"expected_result\": \"what should happen\", \"verification_criteria\": \"how to verify success\"}}\n\
             ]}}",
            soul.system_prompt(),
            tools_description
        );

        let request = ChatRequest::new(
            routing_decision.model,
            vec![
                Message {
                    role: "system".to_string(),
                    content: system_prompt,
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                },
                Message {
                    role: "user".to_string(),
                    content: format!("Create a plan for this task: {}", task),
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                },
            ],
            false,
        );

        let response = self.provider.chat(request).await?;
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        // Try to extract JSON from the response (model may wrap in markdown)
        let json_str = extract_json(&content);
        let plan: Plan = serde_json::from_str(&json_str)
            .with_context(|| format!("Failed to parse plan JSON: {}", content))?;

        self.save_agent_message("assistant", &format!("Generated plan: {:?}", plan))?;

        Ok(plan)
    }

    /// Execute a plan step with retry logic.
    async fn execute_step_with_retry(
        &self,
        step: &PlanStep,
        total_iterations: &mut usize,
    ) -> Result<StepResult> {
        let mut iterations = 0;
        let max_retries = 3;

        loop {
            *total_iterations += 1;
            iterations += 1;

            let output = self.execute_single_step(step, &self.security_engine).await?;
            let verified = self.verify_step(step, &output).await?;

            if verified || iterations >= max_retries {
                return Ok(StepResult {
                    step: step.clone(),
                    output,
                    verified,
                    iterations,
                });
            }

            // Brief pause before retry
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }

    /// Execute a single plan step by invoking the appropriate tool.
    async fn execute_single_step(&self,
        step: &PlanStep,
        security_engine: &crate::security::SecurityEngine,
    ) -> Result<String> {
        // SECURITY GATE: Check before executing
        match security_engine.check_tool_call(&step.tool_name, &step.args) {
            crate::security::SecurityDecision::Allow => {}
            crate::security::SecurityDecision::RequireApproval { reason, risk_level } => {
                return Err(anyhow::anyhow!(
                    "Security approval required for tool '{}': {} (risk: {:?})",
                    step.tool_name, reason, risk_level
                ));
            }
            crate::security::SecurityDecision::Deny { reason } => {
                security_engine.audit(&step.tool_name, &step.args, false,
                    crate::security::RiskLevel::Critical, &reason
                );
                return Err(anyhow::anyhow!(
                    "Security blocked tool '{}': {}",
                    step.tool_name, reason
                ));
            }
        }

        let tool = find_tool(&step.tool_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", step.tool_name))?;

        let result = tool.execute(&step.args)?;
        let sanitized = security_engine.sanitize_output(&step.tool_name, &result);

        let tool_call = ToolCall {
            id: Uuid::new_v4().to_string(),
            session_id: self.session_id.clone(),
            tool_name: step.tool_name.clone(),
            args: step.args.clone(),
            result: sanitized.clone(),
            success: true,
            created_at: Utc::now(),
        };
        self.memory.save_tool_call(&tool_call)?;
        security_engine.audit(&step.tool_name, &step.args, true,
            crate::security::RiskLevel::Low, "approved"
        );

        Ok(sanitized)
    }

    /// Verify if a step's result matches expectations.
    pub async fn verify_step(&self, step: &PlanStep, result: &str) -> Result<bool> {
        // Simple verification: check if result contains expected keywords
        if step.expected_result.is_empty() {
            return Ok(true);
        }

        let expected_lower = step.expected_result.to_lowercase();
        let result_lower = result.to_lowercase();

        // Check for negation indicators in the result
        let negation_words = ["error", "failed", "failure", "not found", "permission denied"];
        let has_negation = negation_words.iter().any(|word| result_lower.contains(word));

        // Check if expected result keywords are present
        let expected_keywords: Vec<&str> = expected_lower.split_whitespace().collect();
        let has_expected = expected_keywords.iter().any(|kw| result_lower.contains(kw));

        Ok(!has_negation && (has_expected || result.len() > 0))
    }

    /// Create a recovery plan when a step fails.
    pub async fn escalate(&self, failed_step: &PlanStep, error: &str) -> Result<Plan> {
        let routing_decision = route_task(
            &self.infer_config(),
            &format!("Recover from failed step: {} - {}", failed_step.tool_name, error),
        )
        .await
        .unwrap_or_else(|_| RoutingDecision {
            task_type: "recovery".to_string(),
            model: self.config.default_model.clone(),
            provider: "local".to_string(),
            reason: "Fallback".to_string(),
        });

        let system_prompt = format!(
            "A tool step failed. Create a recovery plan. \
             Failed step: tool={}, args={}, error={}\n\
             Respond ONLY with a JSON object in this exact format:\n\
             {{\"description\": \"recovery plan\", \"steps\": [\n\
             {{\"tool_name\": \"tool_name\", \"args\": \"arguments\", \"expected_result\": \"what should happen\", \"verification_criteria\": \"how to verify\"}}\n\
             ]}}",
            failed_step.tool_name, failed_step.args, error
        );

        let request = ChatRequest::new(
            routing_decision.model,
            vec![
                Message {
                    role: "system".to_string(),
                    content: system_prompt,
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                },
                Message {
                    role: "user".to_string(),
                    content: "Create a recovery plan.".to_string(),
                    images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                },
            ],
            false,
        );

        let response = self.provider.chat(request).await?;
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        let json_str = extract_json(&content);
        let plan: Plan = serde_json::from_str(&json_str)
            .with_context(|| format!("Failed to parse recovery plan JSON: {}", content))?;

        self.save_agent_message("assistant", &format!("Escalation plan: {:?}", plan))?;

        Ok(plan)
    }

    /// Save a message to the agent's session memory.
    fn save_agent_message(&self, role: &str, content: &str) -> Result<()> {
        let msg = MemoryMessage {
            id: Uuid::new_v4().to_string(),
            session_id: self.session_id.clone(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: Utc::now(),
            tokens_used: None,
        };
        self.memory.save_message(&msg)?;
        Ok(())
    }

    /// Infer a minimal Config for router calls.
    fn infer_config(&self) -> Config {
        Config {
            version: crate::VERSION.to_string(),
            default_model: self.config.default_model.clone(),
            providers: std::collections::HashMap::new(),
            memory_db_path: std::path::PathBuf::from("/tmp/openshark_agent_memory.db"),
            tools_enabled: Vec::new(),
            auto_route: true,
            cost_limit_usd: 10.0,
            agent: crate::config::AgentIdentity::default(),
            gateway: crate::gateway::GatewayConfig::default(),
            user_name: "user".to_string(),
            theme: "synthwave84".to_string(),
            filesystem: crate::config::FilesystemConfig::default(),
            swarm: crate::swarm::SwarmConfig::default(),
            context_compression: crate::memory::compression::ContextCompressionConfig::default(),
        }
    }
}

/// Extract JSON from text that may be wrapped in markdown code blocks.
fn extract_json(text: &str) -> String {
    // Try to find JSON inside markdown code blocks
    if let Some(start) = text.find("```json") {
        if let Some(end) = text[start + 7..].find("```") {
            return text[start + 7..start + 7 + end].trim().to_string();
        }
    }
    if let Some(start) = text.find("```") {
        if let Some(end) = text[start + 3..].find("```") {
            return text[start + 3..start + 3 + end].trim().to_string();
        }
    }
    // Try to find JSON object boundaries
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

/// User approval choices for a plan.
#[derive(Debug, Clone)]
pub enum PlanApproval {
    Approve,
    Edit,
    Reject,
}

/// Prompt the user to approve, edit, or reject a plan.
pub fn prompt_plan_approval(plan: &Plan) -> PlanApproval {
    println!("\n📋 Agent Plan: {}", plan.description);
    println!("Steps:");
    for (i, step) in plan.steps.iter().enumerate() {
        println!(
            "  {}. {} {} → expected: {}",
            i + 1,
            step.tool_name,
            step.args,
            step.expected_result
        );
    }
    println!("\nApprove plan? [y/n/edit]: ");

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return PlanApproval::Reject;
    }

    match input.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => PlanApproval::Approve,
        "edit" | "e" => PlanApproval::Edit,
        _ => PlanApproval::Reject,
    }
}

/// Prompt the user to edit a plan.
pub fn prompt_edit_plan(plan: &Plan) -> Result<Plan> {
    println!("Current plan (JSON):");
    let json = serde_json::to_string_pretty(plan).context("Failed to serialize plan")?;
    println!("{}", json);
    println!("\nEnter edited plan JSON (or press Enter to keep current):");

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("Failed to read edited plan")?;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(plan.clone())
    } else {
        let edited: Plan = serde_json::from_str(trimmed)
            .context("Failed to parse edited plan JSON")?;
        Ok(edited)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_markdown() {
        let text = r#"Some text
```json
{"description": "test", "steps": []}
```
More text"#;
        let result = extract_json(text);
        assert!(result.contains("description"));
        assert!(result.contains("steps"));
    }

    #[test]
    fn test_extract_json_plain() {
        let text = r#"{"description": "test", "steps": []}"#;
        let result = extract_json(text);
        assert_eq!(result, text);
    }

    #[test]
    fn test_plan_step_creation() {
        let step = PlanStep {
            tool_name: "search".to_string(),
            args: "find TODOs".to_string(),
            expected_result: "list of TODOs".to_string(),
            verification_criteria: "non-empty list".to_string(),
        };
        assert_eq!(step.tool_name, "search");
        assert_eq!(step.args, "find TODOs");
    }

    #[test]
    fn test_plan_serialization() {
        let plan = Plan {
            description: "Test plan".to_string(),
            steps: vec![
                PlanStep {
                    tool_name: "search".to_string(),
                    args: "find TODOs".to_string(),
                    expected_result: "list of TODOs".to_string(),
                    verification_criteria: "non-empty list".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("Test plan"));
        assert!(json.contains("search"));

        let deserialized: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.steps.len(), 1);
    }

    #[test]
    fn test_task_result_creation() {
        let result = TaskResult {
            success: true,
            step_results: Vec::new(),
            total_iterations: 0,
            message: "Done".to_string(),
        };
        assert!(result.success);
        assert_eq!(result.message, "Done");
    }

    #[test]
    fn test_agent_config_default() {
        let config = AgentConfig::default();
        assert_eq!(config.max_iterations, MAX_ITERATIONS);
        assert!(config.require_approval);
    }

    #[test]
    fn test_step_result_creation() {
        let step = PlanStep {
            tool_name: "terminal".to_string(),
            args: "cargo test".to_string(),
            expected_result: "tests pass".to_string(),
            verification_criteria: "exit code 0".to_string(),
        };
        let result = StepResult {
            step: step.clone(),
            output: "test result: ok".to_string(),
            verified: true,
            iterations: 1,
        };
        assert!(result.verified);
        assert_eq!(result.iterations, 1);
    }

    #[test]
    fn test_extract_json_no_json() {
        let text = "This has no JSON at all";
        let result = extract_json(text);
        assert_eq!(result, text);
    }

    #[test]
    fn test_plan_approval_enum() {
        let approval = PlanApproval::Approve;
        match approval {
            PlanApproval::Approve => {}
            _ => panic!("Expected Approve"),
        }
    }

    #[test]
    fn test_extract_json_nested_braces() {
        let text = r#"Some text before {"description": "test", "steps": [{"tool_name": "search", "args": "test", "expected_result": "ok", "verification_criteria": "none"}]} and after"#;
        let result = extract_json(text);
        assert!(result.contains("description"));
        assert!(result.contains("steps"));
    }

    #[test]
    fn test_plan_step_multiple_steps() {
        let plan = Plan {
            description: "Multi-step plan".to_string(),
            steps: vec![
                PlanStep {
                    tool_name: "search".to_string(),
                    args: "find bug".to_string(),
                    expected_result: "bug location".to_string(),
                    verification_criteria: "file path found".to_string(),
                },
                PlanStep {
                    tool_name: "edit".to_string(),
                    args: "fix bug".to_string(),
                    expected_result: "code fixed".to_string(),
                    verification_criteria: "no syntax errors".to_string(),
                },
            ],
        };
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[1].tool_name, "edit");
    }

    #[test]
    fn test_task_result_with_steps() {
        let step = PlanStep {
            tool_name: "terminal".to_string(),
            args: "cargo test".to_string(),
            expected_result: "tests pass".to_string(),
            verification_criteria: "exit code 0".to_string(),
        };
        let result = TaskResult {
            success: true,
            step_results: vec![
                StepResult {
                    step: step.clone(),
                    output: "test result: ok".to_string(),
                    verified: true,
                    iterations: 1,
                },
            ],
            total_iterations: 1,
            message: "All good".to_string(),
        };
        assert!(result.success);
        assert_eq!(result.step_results.len(), 1);
        assert_eq!(result.total_iterations, 1);
    }

    #[test]
    fn test_task_result_failure() {
        let result = TaskResult {
            success: false,
            step_results: Vec::new(),
            total_iterations: 0,
            message: "Plan rejected by user.".to_string(),
        };
        assert!(!result.success);
    }

    #[test]
    fn test_max_iterations_constant() {
        assert_eq!(MAX_ITERATIONS, 84);
    }

    #[test]
    fn test_agent_config_custom() {
        let config = AgentConfig {
            max_iterations: 5,
            require_approval: false,
            default_model: "custom-model".to_string(),
        };
        assert_eq!(config.max_iterations, 5);
        assert!(!config.require_approval);
        assert_eq!(config.default_model, "custom-model");
    }

    #[test]
    fn test_step_result_unverified() {
        let step = PlanStep {
            tool_name: "fs".to_string(),
            args: "read missing.txt".to_string(),
            expected_result: "file contents".to_string(),
            verification_criteria: "non-empty".to_string(),
        };
        let result = StepResult {
            step: step.clone(),
            output: "error: file not found".to_string(),
            verified: false,
            iterations: 3,
        };
        assert!(!result.verified);
        assert_eq!(result.iterations, 3);
    }

    #[test]
    fn test_extract_json_code_block_no_lang() {
        let text = r#"```
{"description": "test", "steps": []}
```"#;
        let result = extract_json(text);
        assert!(result.contains("description"));
    }

    #[test]
    fn test_plan_deserialization() {
        let json = r#"{"description":"Fix bug","steps":[{"tool_name":"search","args":"bug","expected_result":"location","verification_criteria":"found"}]}"#;
        let plan: Plan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.description, "Fix bug");
        assert_eq!(plan.steps[0].tool_name, "search");
    }

    #[test]
    fn test_plan_step_empty_args() {
        let step = PlanStep {
            tool_name: "git".to_string(),
            args: "".to_string(),
            expected_result: "status output".to_string(),
            verification_criteria: "no errors".to_string(),
        };
        assert!(step.args.is_empty());
    }

    #[test]
    fn test_plan_approval_reject() {
        let approval = PlanApproval::Reject;
        match approval {
            PlanApproval::Reject => {}
            _ => panic!("Expected Reject"),
        }
    }

    #[test]
    fn test_plan_approval_edit() {
        let approval = PlanApproval::Edit;
        match approval {
            PlanApproval::Edit => {}
            _ => panic!("Expected Edit"),
        }
    }
}
