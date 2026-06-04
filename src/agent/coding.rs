//! Coding Agent — Autonomous plan → edit → test → commit loop
//!
//! Tier 1 Git Agent feature. Given a task like "fix the auth bug",
//! the agent:
//!   1. Plans the approach (search → edit → test → commit)
//!   2. Executes each step with the tool registry
//!   3. Runs tests after edits
//!   4. Auto-commits on success (if enabled)
//!   5. Reports results back to the TUI

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::agent::{AgentConfig, Plan, PlanStep, StepResult, TaskResult};
use crate::config::Config;
use crate::memory::{MemoryStore, Message as MemoryMessage, ToolCall};
use crate::providers::{ChatRequest, Message, Provider};
use crate::router::{RoutingDecision, route_task};
use crate::tools::{find_tool, get_tools};

/// A coding-specific agent that runs the plan/edit/test/commit loop.
pub struct CodingAgent {
    config: AgentConfig,
    memory: MemoryStore,
    provider: Provider,
    session_id: String,
    security_engine: crate::security::SecurityEngine,
    app_config: Config,
}

/// Progress update sent during agent execution.
#[derive(Debug, Clone)]
pub enum AgentProgress {
    PlanGenerated { description: String, steps: usize },
    StepStarted { idx: usize, total: usize, tool: String, args: String },
    StepCompleted { idx: usize, output: String, verified: bool },
    TestsRunning { framework: String },
    TestsCompleted { passed: usize, failed: usize, output: String },
    LintRunning { tool: String },
    LintCompleted { issues: usize, output: String },
    Committing { message: String },
    Committed { output: String },
    Error(String),
    Done { success: bool, message: String },
}

impl CodingAgent {
    /// Create a new coding agent.
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
            crate::security::SecurityConfig::load().unwrap_or_default(),
        )?;

        memory.create_session(&session_id, model_name, "coding")?;

        Ok(Self {
            config,
            memory,
            provider,
            session_id,
            security_engine,
            app_config: app_config.clone(),
        })
    }

    /// Run the full coding loop with progress callbacks.
    pub async fn run_coding_task<F>(
        &self,
        task: &str,
        mut progress: F,
    ) -> Result<TaskResult>
    where
        F: FnMut(AgentProgress),
    {
        self.save_message("system", &format!("Starting coding task: {}", task))?;

        // ── Phase 1: Generate plan ──────────────────────────────────────────
        progress(AgentProgress::PlanGenerated {
            description: "Generating plan...".to_string(),
            steps: 0,
        });

        let plan = self.generate_coding_plan(task).await?;

        progress(AgentProgress::PlanGenerated {
            description: plan.description.clone(),
            steps: plan.steps.len(),
        });

        let mut step_results: Vec<StepResult> = Vec::new();
        let mut total_iterations: usize = 0;
        let mut tests_run = false;
        let mut lint_run = false;

        // ── Phase 2: Execute plan ───────────────────────────────────────────
        for (step_idx, step) in plan.steps.iter().enumerate() {
            if total_iterations >= self.config.max_iterations {
                progress(AgentProgress::Error(format!(
                    "Reached maximum iterations ({}). Stopping.",
                    self.config.max_iterations
                )));
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

            progress(AgentProgress::StepStarted {
                idx: step_idx + 1,
                total: plan.steps.len(),
                tool: step.tool_name.clone(),
                args: step.args.clone(),
            });

            let step_result = self
                .execute_step_with_retry(step, &mut total_iterations)
                .await?;

            progress(AgentProgress::StepCompleted {
                idx: step_idx + 1,
                output: step_result.output.clone(),
                verified: step_result.verified,
            });

            step_results.push(step_result.clone());

            self.save_message(
                "assistant",
                &format!(
                    "Step {}: tool={}, verified={}, iterations={}",
                    step_idx + 1,
                    step_result.step.tool_name,
                    step_result.verified,
                    step_result.iterations
                ),
            )?;

            // ── Phase 3: Run tests after edit tools ─────────────────────────
            if is_edit_tool(&step.tool_name) && !tests_run {
                if let Some(test_result) = self.run_tests().await {
                    progress(AgentProgress::TestsRunning {
                        framework: test_result.framework.clone(),
                    });
                    progress(AgentProgress::TestsCompleted {
                        passed: test_result.passed,
                        failed: test_result.failed,
                        output: format_test_result(&test_result),
                    });
                    tests_run = true;

                    // If tests failed, try to fix
                    if test_result.failed > 0 && total_iterations < self.config.max_iterations {
                        let fix_plan = self.generate_fix_plan(&test_result).await?;
                        if !fix_plan.steps.is_empty() {
                            progress(AgentProgress::PlanGenerated {
                                description: format!("Auto-fix: {}", fix_plan.description),
                                steps: fix_plan.steps.len(),
                            });
                            for fix_step in &fix_plan.steps {
                                if total_iterations >= self.config.max_iterations {
                                    break;
                                }
                                let fix_result = self
                                    .execute_step_with_retry(fix_step, &mut total_iterations)
                                    .await?;
                                step_results.push(fix_result);
                            }
                        }
                    }
                }
            }

            // ── Phase 4: Run linter after edits ─────────────────────────────
            if is_edit_tool(&step.tool_name) && !lint_run && self.app_config.auto_lint {
                progress(AgentProgress::LintRunning {
                    tool: "auto-detected".to_string(),
                });
                match crate::linting::run_linter(".").await {
                    Ok(results) => {
                        let issues = results.len();
                        progress(AgentProgress::LintCompleted {
                            issues,
                            output: crate::linting::format_lint_results(&results),
                        });
                        lint_run = true;

                        // If lint errors exist and we have iterations left, try to fix
                        if issues > 0 && total_iterations < self.config.max_iterations {
                            let lint_fix = self.generate_lint_fix_plan(&results).await?;
                            if !lint_fix.steps.is_empty() {
                                progress(AgentProgress::PlanGenerated {
                                    description: format!("Lint fix: {}", lint_fix.description),
                                    steps: lint_fix.steps.len(),
                                });
                                for lint_step in &lint_fix.steps {
                                    if total_iterations >= self.config.max_iterations {
                                        break;
                                    }
                                    let lr = self
                                        .execute_step_with_retry(lint_step, &mut total_iterations)
                                        .await?;
                                    step_results.push(lr);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        progress(AgentProgress::Error(format!("Lint failed: {}", e)));
                    }
                }
            }
        }

        // ── Phase 5: Auto-commit if enabled and edits were made ─────────────
        let edits_made = step_results.iter().any(|r| is_edit_tool(&r.step.tool_name));
        if edits_made && self.app_config.auto_commit {
            match self.auto_commit().await {
                Ok(commit_output) => {
                    progress(AgentProgress::Committed {
                        output: commit_output,
                    });
                }
                Err(e) => {
                    progress(AgentProgress::Error(format!("Auto-commit failed: {}", e)));
                }
            }
        }

        let all_verified = step_results.iter().all(|r| r.verified);
        let message = if all_verified {
            "Task completed successfully.".to_string()
        } else {
            "Task completed with some unverified steps.".to_string()
        };

        progress(AgentProgress::Done {
            success: all_verified,
            message: message.clone(),
        });

        Ok(TaskResult {
            success: all_verified,
            step_results,
            total_iterations,
            message,
        })
    }

    /// Generate a coding-specific plan with test and commit steps.
    async fn generate_coding_plan(&self, task: &str) -> Result<Plan> {
        let routing_decision = route_task(&self.app_config, task)
            .await
            .unwrap_or_else(|_| RoutingDecision {
                task_type: "coding".to_string(),
                model: self.config.default_model.clone(),
                provider: "local".to_string(),
                reason: "Fallback".to_string(),
            });

        let tools_description = get_tools()
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = format!(
            "You are an autonomous coding agent. Break down coding tasks into concrete steps. \
             Each step must use one of these tools:\n{}\n\
             CRITICAL: Every plan step must have a clear expected_result and verification_criteria. \
             Include test steps after edits. Include a commit step if changes are made. \
             Respond ONLY with a JSON object in this exact format:\n\
             {{\"description\": \"brief plan description\", \"steps\": [\n\
             {{\"tool_name\": \"tool_name\", \"args\": \"arguments\", \"expected_result\": \"what should happen\", \"verification_criteria\": \"how to verify success\"}}\n\
             ]}}",
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
                    content: format!("Create a plan for this coding task: {}", task),
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

        let json_str = crate::agent::extract_json(&content);
        let plan: Plan = serde_json::from_str(&json_str)
            .with_context(|| format!("Failed to parse plan JSON: {}", content))?;

        self.save_message("assistant", &format!("Generated plan: {:?}", plan))?;

        Ok(plan)
    }

    /// Generate a recovery plan for failed tests.
    async fn generate_fix_plan(
        &self,
        test_result: &crate::tools::test_runner::TestResultSet,
    ) -> Result<Plan> {
        let failures = test_result
            .failures
            .iter()
            .map(|f| format!("- {}: {}", f.test_name, f.message))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Tests failed. Create a fix plan.\n\nFailed tests:\n{}\n\nTest output:\n{}\n\nRespond with JSON plan.",
            failures,
            test_result.raw_output.chars().take(2000).collect::<String>()
        );

        self.generate_plan_from_prompt(&prompt, "test-fix").await
    }

    /// Generate a fix plan for lint errors.
    async fn generate_lint_fix_plan(
        &self,
        lint_results: &[crate::linting::LintResult],
    ) -> Result<Plan> {
        let issues = lint_results
            .iter()
            .map(|r| format!("- {}:{} — {}", r.file, r.line, r.message))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Lint errors found. Create a fix plan.\n\nIssues:\n{}\n\nRespond with JSON plan.",
            issues
        );

        self.generate_plan_from_prompt(&prompt, "lint-fix").await
    }

    /// Generic plan generation from a prompt.
    async fn generate_plan_from_prompt(&self, prompt: &str, plan_type: &str) -> Result<Plan> {
        let system_prompt = format!(
            "Create a concise {} plan. Each step must use one tool with clear expected_result and verification_criteria. \
             Respond ONLY with JSON: {{\"description\":\"...\",\"steps\":[{{\"tool_name\":\"...\",\"args\":\"...\",\"expected_result\":\"...\",\"verification_criteria\":\"...\"}}]}}",
            plan_type
        );

        let request = ChatRequest::new(
            self.config.default_model.clone(),
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
                    content: prompt.to_string(),
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

        let json_str = crate::agent::extract_json(&content);
        let plan: Plan = serde_json::from_str(&json_str)
            .with_context(|| format!("Failed to parse {} plan JSON: {}", plan_type, content))?;

        Ok(plan)
    }

    /// Execute a step with retry logic.
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

            let output = self.execute_single_step(step).await?;
            let verified = self.verify_step(step, &output).await?;

            if verified || iterations >= max_retries {
                return Ok(StepResult {
                    step: step.clone(),
                    output,
                    verified,
                    iterations,
                });
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }

    /// Execute a single tool step.
    async fn execute_single_step(&self, step: &PlanStep) -> Result<String> {
        match self.security_engine.check_tool_call(&step.tool_name, &step.args) {
            crate::security::SecurityDecision::Allow => {}
            crate::security::SecurityDecision::RequireApproval { reason, risk_level } => {
                return Err(anyhow::anyhow!(
                    "Security approval required for tool '{}': {} (risk: {:?})",
                    step.tool_name,
                    reason,
                    risk_level
                ));
            }
            crate::security::SecurityDecision::Deny { reason } => {
                self.security_engine.audit(
                    &step.tool_name,
                    &step.args,
                    false,
                    crate::security::RiskLevel::Critical,
                    &reason,
                );
                return Err(anyhow::anyhow!(
                    "Security blocked tool '{}': {}",
                    step.tool_name,
                    reason
                ));
            }
        }

        let result = if let Some(async_tool) = crate::tools::find_async_tool(&step.tool_name) {
            async_tool.execute_async(&step.args).await?
        } else {
            let tool = find_tool(&step.tool_name)
                .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", step.tool_name))?;
            tool.execute(&step.args)?
        };
        let sanitized = self.security_engine.sanitize_output(&step.tool_name, &result);

        let tool_call = ToolCall {
            id: Uuid::new_v4().to_string(),
            session_id: self.session_id.clone(),
            tool_name: step.tool_name.clone(),
            args: step.args.clone(),
            result: sanitized.clone(),
            success: true,
            created_at: Utc::now(),
        };
        let _ = self.memory.save_tool_call(&tool_call);
        self.security_engine.audit(
            &step.tool_name,
            &step.args,
            true,
            crate::security::RiskLevel::Low,
            "approved",
        );

        Ok(sanitized)
    }

    /// Verify a step result.
    async fn verify_step(&self, step: &PlanStep, result: &str) -> Result<bool> {
        if step.expected_result.is_empty() {
            return Ok(true);
        }

        let expected_lower = step.expected_result.to_lowercase();
        let result_lower = result.to_lowercase();

        let negation_words = ["error", "failed", "failure", "not found", "permission denied"];
        let has_negation = negation_words.iter().any(|word| result_lower.contains(word));
        let has_expected = expected_lower
            .split_whitespace()
            .any(|kw| result_lower.contains(kw));

        Ok(!has_negation && (has_expected || !result.is_empty()))
    }

    /// Run tests and return structured results.
    async fn run_tests(&self) -> Option<crate::tools::test_runner::TestResultSet> {
        let project_path = self
            .app_config
            .filesystem
            .working_directory
            .clone()
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))?;

        match crate::tools::test_runner::run_tests_structured(
            &project_path,
        ) {
            Ok(result) => Some(result),
            Err(e) => {
                let _ = self.save_message("system", &format!("Test run failed: {}", e));
                None
            }
        }
    }

    /// Auto-commit changes with a generated message.
    async fn auto_commit(&self) -> Result<String> {
        let git_tool = crate::tools::GitTool;

        if !crate::tools::GitTool::in_repo() {
            anyhow::bail!("Not in a git repository");
        }

        if !crate::tools::GitTool::has_changes() {
            return Ok("No changes to commit".to_string());
        }

        // Stage all
        crate::tools::Tool::execute(&git_tool, "stage-all")?;

        // Generate commit message from diff
        let diff = crate::tools::Tool::execute(&git_tool, "diff --staged")?;
        let commit_msg = if diff.trim().is_empty() {
            "chore: auto-commit".to_string()
        } else {
            self.generate_commit_msg(&diff).await.unwrap_or_else(|_| {
                format!("chore: auto-commit at {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
            })
        };

        let output = crate::tools::Tool::execute(&git_tool, &format!("commit {}", commit_msg))?;
        Ok(format!("Committed: {}\n{}", commit_msg, output))
    }

    /// Generate a commit message from a diff.
    async fn generate_commit_msg(&self, diff: &str) -> Result<String> {
        let prompt = format!(
            "Generate a concise conventional commit message for this diff. \
             Format: type(scope): description. Types: feat, fix, docs, style, refactor, test, chore. \
             Max 72 chars for first line.\n\n```diff\n{}\n```",
            diff.chars().take(4000).collect::<String>()
        );

        let request = ChatRequest::new(
            self.config.default_model.clone(),
            vec![
                Message {
                    role: "system".to_string(),
                    content: "You generate concise conventional commit messages.".to_string(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                Message {
                    role: "user".to_string(),
                    content: prompt,
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
            ],
            false,
        );

        let response = self.provider.chat(request).await?;
        let msg = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_else(|| "chore: update".to_string());

        let msg = msg.lines().next().unwrap_or("chore: update").trim().to_string();
        let msg = msg.trim_start_matches("Commit message:").trim().to_string();
        let msg = msg.trim_start_matches('"').trim_end_matches('"').to_string();

        if msg.is_empty() {
            Ok("chore: update".to_string())
        } else {
            Ok(msg)
        }
    }

    /// Save a message to agent memory.
    fn save_message(&self, role: &str, content: &str) -> Result<()> {
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
}

/// Check if a tool name is an edit tool.
fn is_edit_tool(name: &str) -> bool {
    matches!(
        name,
        "edit" | "write" | "write_file" | "patch" | "refactor" | "replace"
    )
}

/// Format test results for display.
fn format_test_result(result: &crate::tools::test_runner::TestResultSet) -> String {
    let status = if result.success { "✅ PASSED" } else { "❌ FAILED" };
    format!(
        "{} | {} passed, {} failed, {} ignored ({} total)",
        status, result.passed, result.failed, result.ignored, result.total
    )
}
