//! Headless / CI-CD mode for OpenShark
//!
//! Run OpenShark non-interactively: `openshark --autonomous "implement feature X"`
//! Outputs structured JSON or plain text for piping into other tools.
//!
//! Features:
//!   --yolo          Auto-approve all tool calls (no interactive prompts)
//!   --autonomous    Full autonomy mode: no approvals, auto-commit, auto-test
//!   --json          Output NDJSON for structured consumption
//!   --timeout       Max seconds to run (default: 300)
//!   --max-turns     Max agent turns (default: 50)
//!   --model         Override model for this run
//!   --output        Write output to file instead of stdout

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::tools::{
    detect_tool_suggestions, execute_tool, find_async_tool, get_openai_tool_definitions,
};
use crate::providers::{ChatRequest, Message};
use crate::security::SecurityEngine;

/// Structured output event for --json mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HeadlessEvent {
    Start {
        task: String,
        model: String,
        timestamp: String,
    },
    Thought {
        content: String,
        turn: usize,
        timestamp: String,
    },
    ToolCall {
        name: String,
        args: String,
        turn: usize,
        timestamp: String,
    },
    ToolResult {
        name: String,
        output: String,
        success: bool,
        turn: usize,
        timestamp: String,
    },
    Error {
        message: String,
        turn: usize,
        timestamp: String,
    },
    Complete {
        summary: String,
        total_turns: usize,
        duration_secs: u64,
        timestamp: String,
    },
}

/// Headless execution configuration.
#[derive(Debug, Clone)]
pub struct HeadlessConfig {
    pub task: String,
    pub yolo: bool,
    /// Full autonomy mode — bypasses all approvals, auto-commits, auto-tests.
    pub autonomous: bool,
    pub json: bool,
    pub timeout_secs: u64,
    pub max_turns: usize,
    pub model: Option<String>,
    pub output_file: Option<String>,
}

impl Default for HeadlessConfig {
    fn default() -> Self {
        Self {
            task: String::new(),
            yolo: false,
            autonomous: false,
            json: false,
            timeout_secs: 300,
            max_turns: 50,
            model: None,
            output_file: None,
        }
    }
}

/// Build the system prompt for headless mode with tool descriptions.
fn build_system_prompt(autonomous: bool) -> String {
    let tool_descs = crate::tools::get_all_tool_descriptions();
    let tool_list: Vec<String> = tool_descs
        .iter()
        .map(|(name, desc)| format!("  - {}: {}", name, desc))
        .collect();

    format!(
        "You are OpenShark, an autonomous AI coding agent running in headless mode.\n\
         Your task will be given as a user message. Execute it by using tools.\n\
         {}\n\n\
         ## Available Tools\n\
         You are running in a native tool-calling environment.\n\
         When you need to use a tool, you MUST use the `tool_calls` function_calling format.\n\
         Do NOT output `TOOL:` patterns or markdown code blocks — the system will call tools for you.\n\
         Wait for tool results after each call, then decide the next step.\n\n\
         ## Tool Reference\n\
         {}\n\n\
         ## Rules\n\
         - Execute tools to accomplish the task. Do not just describe what you would do.\n\
         - After each tool result, decide what to do next.\n\
         - When the task is complete, respond with TASK_COMPLETE on its own line.\n\
         - If you encounter an error, try to fix it. If unfixable, explain and respond TASK_COMPLETE.\n\
         - Be concise. Focus on the task.\n\
         - You can use multiple tools in a single turn if they are independent.",
        if autonomous { "🤖 AUTONOMOUS MODE: You are fully autonomous. No human approval needed. Make decisions and commit changes." } else { "🛡️ GUARDED MODE: All tools must pass security checks." },
        tool_list.join("\n")
    )
}

/// Run a task in headless mode with full tool execution.
/// Returns the final summary string.
pub async fn run_headless(
    config: HeadlessConfig,
    provider: crate::providers::Provider,
    model: String,
    security: SecurityEngine,
    event_tx: Option<mpsc::UnboundedSender<HeadlessEvent>>,
) -> anyhow::Result<String> {
    let start = Instant::now();
    let mut summary = String::new();
    let mut turn = 0usize;
    let mut consecutive_errors = 0usize;
    let max_consecutive_errors = 3;
    let is_autonomous = config.autonomous || config.yolo;

    let now = || chrono::Utc::now().to_rfc3339();

    emit(&event_tx, HeadlessEvent::Start {
        task: config.task.clone(),
        model: model.clone(),
        timestamp: now(),
    });

    let system_prompt = build_system_prompt(is_autonomous);
    let mut messages: Vec<Message> = vec![
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
            content: config.task.clone(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        },
    ];

    // Inject repo context if available
    if let Some(context) = load_repo_context(&config.task) {
        messages.insert(
            1, // Insert after system prompt, before user task
            Message {
                role: "user".to_string(),
                content: format!("Here is the repository context for this task:\n\n{}", context),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
        );
    }

    // Create a checkpoint before autonomous execution
    if is_autonomous && crate::tools::checkpoint::in_git_repo() {
        match crate::tools::checkpoint::save_checkpoint(&format!("pre-autonomous-{}", chrono::Utc::now().timestamp()))
        {
            Ok(cp) => {
                emit(&event_tx, HeadlessEvent::Thought {
                    content: format!("💾 Checkpoint created: {} ({})", cp.name, cp.git_ref),
                    turn: 0,
                    timestamp: now(),
                });
            }
            Err(e) => {
                emit(&event_tx, HeadlessEvent::Error {
                    message: format!("⚠️ Checkpoint creation failed: {}", e),
                    turn: 0,
                    timestamp: now(),
                });
            }
        }
    }

    let mut recent_tool_calls: Vec<(String, String, usize, bool)> = Vec::new();
    let mut stall_turns: usize = 0;

    // Get tool definitions for native function calling
    let tool_definitions = get_openai_tool_definitions();
    let tools_enabled = !tool_definitions.is_empty();

    while turn < config.max_turns {
        if start.elapsed() > Duration::from_secs(config.timeout_secs) {
            let msg = format!("Timeout after {} seconds", config.timeout_secs);
            emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
            summary = msg;
            break;
        }

        turn += 1;

        let request = ChatRequest {
            model: model.clone(),
            messages: messages.clone(),
            stream: false,
            max_tokens: None,
            temperature: None,
            tools: if tools_enabled { Some(tool_definitions.clone()) } else { None },
        };

        let response = match provider.chat(request).await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("Provider error: {}", e);
                emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                consecutive_errors += 1;
                if consecutive_errors >= max_consecutive_errors {
                    summary = msg;
                    break;
                }
                // Brief pause before retry
                tokio::time::sleep(Duration::from_secs(1)).await;
                messages.push(Message {
                    role: "user".to_string(),
                    content: format!("The previous API request failed: {}. Please try again or use a different approach.", e),
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                });
                continue;
            }
        };

        let choice = response.choices.first();
        let assistant_message = choice.map(|c| c.message.clone()).unwrap_or_default();
        let finish_reason = choice.and_then(|c| c.finish_reason.clone()).unwrap_or_default();
        let response_text = assistant_message.content.clone();

        emit(&event_tx, HeadlessEvent::Thought {
            content: response_text.clone(),
            turn,
            timestamp: now(),
        });

        consecutive_errors = 0; // Reset on successful response

        // Check for task completion signals
        if response_text.contains("TASK_COMPLETE")
            || response_text.contains("Done!")
            || response_text.contains("All done")
            || finish_reason == "stop"
        {
            summary = response_text;
            break;
        }

        // ── NATIVE TOOL CALLING PATH ─────────────────────────────────────
        if let Some(ref tool_calls) = assistant_message.tool_calls {
            if !tool_calls.is_empty() {
                // Add assistant message with tool_calls to history
                messages.push(Message {
                    role: "assistant".to_string(),
                    content: response_text.clone(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: Some(tool_calls.clone()),
                    reasoning_content: None,
                });

                let mut tool_results = String::new();
                for tc in tool_calls {
                    let tool_name = &tc.function.name;
                    let args = &tc.function.arguments;
                    let call_id = &tc.id;

                    emit(&event_tx, HeadlessEvent::ToolCall {
                        name: tool_name.clone(),
                        args: args.clone(),
                        turn,
                        timestamp: now(),
                    });

                    // Security gate (autonomous mode bypasses)
                    if !is_autonomous {
                        match security.check_tool_call(tool_name, args) {
                            crate::security::SecurityDecision::Allow => {}
                            crate::security::SecurityDecision::RequireApproval { reason, .. } => {
                                let msg = format!("🔒 Tool '{}' requires approval: {}", tool_name, reason);
                                emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                                messages.push(Message {
                                    role: "tool".to_string(),
                                    content: msg,
                                    images: None,
                                    tool_call_id: Some(call_id.clone()),
                                    tool_calls: None,
                                    reasoning_content: None,
                                });
                                continue;
                            }
                            crate::security::SecurityDecision::Deny { reason } => {
                                let msg = format!("🚫 Tool '{}' blocked: {}", tool_name, reason);
                                emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                                messages.push(Message {
                                    role: "tool".to_string(),
                                    content: msg,
                                    images: None,
                                    tool_call_id: Some(call_id.clone()),
                                    tool_calls: None,
                                    reasoning_content: None,
                                });
                                continue;
                            }
                        }
                    }

                    // Loop detection
                    let recent_failures = recent_tool_calls
                        .iter()
                        .rev()
                        .take(5)
                        .filter(|(name, a, _, success)| !success && name == tool_name && a == args)
                        .count();
                    if recent_failures >= 3 {
                        let msg = format!("🔄 Loop detected: {}({}) failed {} times. Breaking.", tool_name, args, recent_failures);
                        emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                        messages.push(Message {
                            role: "tool".to_string(),
                            content: msg,
                            images: None,
                            tool_call_id: Some(call_id.clone()),
                            tool_calls: None,
                            reasoning_content: None,
                        });
                        continue;
                    }

                    // Execute the tool
                    let (result, success) = execute_tool_call(tool_name, args, &security, &event_tx, turn).await;

                    recent_tool_calls.push((tool_name.clone(), args.clone(), turn, success));
                    tool_results.push_str(&format!("{}\n", result));

                    messages.push(Message {
                        role: "tool".to_string(),
                        content: result,
                        images: None,
                        tool_call_id: Some(call_id.clone()),
                        tool_calls: None,
                        reasoning_content: None,
                    });
                }
                summary.push_str(&tool_results);
                stall_turns = 0;
                continue;
            }
        }

        // ── FALLBACK: TEXT-BASED TOOL DETECTION ───────────────────────────
        let suggestions = detect_tool_suggestions(&response_text);

        if suggestions.is_empty() {
            stall_turns += 1;
            if stall_turns >= 3 {
                let msg = "Loop detected: model stalled with no tools or completion for 3 turns. Breaking.".to_string();
                emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                summary.push_str(&msg);
                summary.push('\n');
                break;
            }
            messages.push(Message {
                role: "assistant".to_string(),
                content: response_text.clone(),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
            messages.push(Message {
                role: "user".to_string(),
                content: "Continue working on the task. If finished, respond with TASK_COMPLETE. If you need to use a tool, use the native tool_calls format.".to_string(),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
            summary.push_str(&response_text);
            summary.push('\n');
            continue;
        }
        stall_turns = 0;

        // Add assistant message (with tool calls) to history
        messages.push(Message {
            role: "assistant".to_string(),
            content: response_text.clone(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        // Execute each detected tool
        let mut tool_results = String::new();
        for suggestion in &suggestions {
            emit(&event_tx, HeadlessEvent::ToolCall {
                name: suggestion.tool_name.clone(),
                args: suggestion.args.clone(),
                turn,
                timestamp: now(),
            });

            // Security gate (in yolo mode, auto-approve; otherwise check)
            if !is_autonomous {
                match security.check_tool_call(&suggestion.tool_name, &suggestion.args) {
                    crate::security::SecurityDecision::Allow => {}
                    crate::security::SecurityDecision::RequireApproval { reason, .. } => {
                        let msg = format!(
                            "🔒 Tool '{}' requires approval (non-yolo mode): {}",
                            suggestion.tool_name, reason
                        );
                        emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                        tool_results.push_str(&msg);
                        tool_results.push('\n');
                        continue;
                    }
                    crate::security::SecurityDecision::Deny { reason } => {
                        let msg = format!("🚫 Tool '{}' blocked: {}", suggestion.tool_name, reason);
                        emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                        tool_results.push_str(&msg);
                        tool_results.push('\n');
                        continue;
                    }
                }
            }

            let recent_failures = recent_tool_calls
                .iter()
                .rev()
                .take(5)
                .filter(|(name, args, _, success)| {
                    !success && name == &suggestion.tool_name && args == &suggestion.args
                })
                .count();

            let recent_successes = recent_tool_calls
                .iter()
                .rev()
                .take(10)
                .filter(|(name, args, _, success)| {
                    *success && name == &suggestion.tool_name && args == &suggestion.args
                })
                .count();

            if recent_failures >= 3 {
                let msg = format!(
                    "🔄 Loop detected: {}({}) has failed {} times recently. Breaking to prevent infinite loop.",
                    suggestion.tool_name, suggestion.args, recent_failures
                );
                emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                tool_results.push_str(&msg);
                tool_results.push('\n');
                summary.push_str(&tool_results);
                break;
            }

            if recent_successes >= 2 {
                let msg = format!(
                    "🔄 Loop detected: {}({}) has already succeeded {} times. Using cached result to prevent infinite loop.",
                    suggestion.tool_name, suggestion.args, recent_successes
                );
                emit(&event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
                tool_results.push_str(&msg);
                tool_results.push('\n');
                recent_tool_calls.push((
                    suggestion.tool_name.clone(),
                    suggestion.args.clone(),
                    turn,
                    true,
                ));
                continue;
            }

            // Find and execute the tool — try async first, then sync
            let (result, success) = execute_tool_call(
                &suggestion.tool_name,
                &suggestion.args,
                &security,
                &event_tx,
                turn,
            ).await;

            recent_tool_calls.push((
                suggestion.tool_name.clone(),
                suggestion.args.clone(),
                turn,
                success,
            ));

            tool_results.push_str(&result);
            tool_results.push('\n');
        }

        // Feed tool results back as a user message
        messages.push(Message {
            role: "user".to_string(),
            content: format!("Tool results:\n{}", tool_results),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        summary.push_str(&tool_results);
    }

    // ── POST-COMPLETION: AUTO-VERIFY, AUTO-COMMIT, SELF-IMPROVE ───────
    if is_autonomous && !summary.is_empty() && !summary.contains("Loop detected") {
        // Auto-test
        match auto_run_tests().await {
            Ok(test_result) => {
                emit(&event_tx, HeadlessEvent::Thought {
                    content: test_result.clone(),
                    turn: turn + 1,
                    timestamp: now(),
                });
                summary.push('\n');
                summary.push_str(&test_result);
            }
            Err(e) => {
                let msg = format!("⚠️ Auto-test failed: {}", e);
                emit(&event_tx, HeadlessEvent::Error {
                    message: msg.clone(),
                    turn: turn + 1,
                    timestamp: now(),
                });
                summary.push('\n');
                summary.push_str(&msg);
            }
        }

        // Auto-commit
        match auto_commit_changes(&config.task).await {
            Ok(commit_result) => {
                emit(&event_tx, HeadlessEvent::Thought {
                    content: commit_result.clone(),
                    turn: turn + 1,
                    timestamp: now(),
                });
                summary.push('\n');
                summary.push_str(&commit_result);
            }
            Err(e) => {
                let msg = format!("⚠️ Auto-commit failed: {}", e);
                emit(&event_tx, HeadlessEvent::Error {
                    message: msg.clone(),
                    turn: turn + 1,
                    timestamp: now(),
                });
                summary.push('\n');
                summary.push_str(&msg);
            }
        }
        // Trigger self-improvement analysis in background
        tokio::spawn(async move {
            if let Err(e) = crate::self_improve::trigger_analysis(
                &crate::config::Config::load_or_default().unwrap_or_default()
            ).await {
                tracing::warn!("Self-improvement analysis failed: {}", e);
            }
        });
    }

    let duration = start.elapsed().as_secs();

    // Write output to file if requested
    if let Some(ref path) = config.output_file {
        if let Err(e) = tokio::fs::write(path, &summary).await {
            let msg = format!("⚠️ Failed to write output to {}: {}", path, e);
            emit(&event_tx, HeadlessEvent::Error {
                message: msg.clone(),
                turn: turn + 1,
                timestamp: now(),
            });
            summary.push('\n');
            summary.push_str(&msg);
        }
    }

    emit(&event_tx, HeadlessEvent::Complete {
        summary: summary.clone(),
        total_turns: turn,
        duration_secs: duration,
        timestamp: now(),
    });

    Ok(summary)
}

/// Helper: emit an event if a channel is available.
fn emit(tx: &Option<mpsc::UnboundedSender<HeadlessEvent>>, event: HeadlessEvent) {
    if let Some(sender) = tx {
        let _ = sender.send(event);
    }
}

/// Execute a single tool call (async or sync) with security sanitization.
async fn execute_tool_call(
    tool_name: &str,
    args: &str,
    security: &SecurityEngine,
    event_tx: &Option<mpsc::UnboundedSender<HeadlessEvent>>,
    turn: usize,
) -> (String, bool) {
    let now = || chrono::Utc::now().to_rfc3339();
    if let Some(async_tool) = find_async_tool(tool_name) {
        match async_tool.execute_async(args).await {
            Ok(output) => {
                let sanitized = security.sanitize_output(tool_name, &output);
                emit(event_tx, HeadlessEvent::ToolResult {
                    name: tool_name.to_string(),
                    output: sanitized.clone(),
                    success: true,
                    turn,
                    timestamp: now(),
                });
                (
                    format!("✅ {} ({}): {}", tool_name, args, sanitized),
                    true,
                )
            }
            Err(e) => {
                let msg = format!("❌ {} failed: {}", tool_name, e);
                emit(event_tx, HeadlessEvent::ToolResult {
                    name: tool_name.to_string(),
                    output: msg.clone(),
                    success: false,
                    turn,
                    timestamp: now(),
                });
                (msg, false)
            }
        }
    } else if let Some(output) = execute_tool(tool_name, args) {
        match output {
            Ok(output) => {
                let sanitized = security.sanitize_output(tool_name, &output);
                emit(event_tx, HeadlessEvent::ToolResult {
                    name: tool_name.to_string(),
                    output: sanitized.clone(),
                    success: true,
                    turn,
                    timestamp: now(),
                });
                (
                    format!("✅ {} ({}): {}", tool_name, args, sanitized),
                    true,
                )
            }
            Err(e) => {
                let msg = format!("❌ {} failed: {}", tool_name, e);
                emit(event_tx, HeadlessEvent::ToolResult {
                    name: tool_name.to_string(),
                    output: msg.clone(),
                    success: false,
                    turn,
                    timestamp: now(),
                });
                (msg, false)
            }
        }
    } else {
        let msg = format!("❌ Unknown tool: {}", tool_name);
        emit(event_tx, HeadlessEvent::Error { message: msg.clone(), turn, timestamp: now() });
        (msg, false)
    }
}

/// Load repository context (repo map + relevant files) if inside a git repo.
fn load_repo_context(task: &str) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let git_dir = cwd.join(".git");
    if !git_dir.exists() {
        return None;
    }

    let repo_map = crate::repo_map::build_repo_map(&cwd.to_string_lossy()).ok()?;
    let mut context = format!(
        "📁 Repository Context: {}\n\n",
        cwd.display()
    );
    context.push_str(&crate::repo_map::format_repo_map(&repo_map));
    context.push('\n');

    // Try to find relevant files for the task
    let mut engine = crate::context_mode::ContextModeEngine::new(cwd.to_string_lossy().to_string());
    let _ = engine.refresh_cache();
    let relevant = engine.identify_relevant_files(task);
    if !relevant.is_empty() {
        context.push_str("\n🔍 Relevant Files for this Task:\n");
        for file in relevant.iter().take(10) {
            context.push_str(&format!("  - {} (score: {:.2})\n", file.path, file.score));
            if let Ok(content) = std::fs::read_to_string(&file.path) {
                let lines: Vec<&str> = content.lines().collect();
                let preview = lines.iter().take(20).cloned().collect::<Vec<_>>().join("\n");
                context.push_str(&format!("```{}\n{}\n```\n", file.path, preview));
            }
        }
    }

    Some(context)
}

/// Auto-commit any changes after a successful autonomous run.
async fn auto_commit_changes(task: &str) -> anyhow::Result<String> {
    use crate::tools::git::GitTool;
    use crate::tools::Tool;

    let git = GitTool;

    // Check if there are changes
    let status = git.execute("status")?;
    if !status.contains("modified") && !status.contains("Untracked") && !status.contains("new file") {
        return Ok("No changes to commit.".to_string());
    }

    // Stage all changes
    let _ = git.execute("add .")?;

    // Generate commit message from task
    let commit_msg = format!(
        "openshark: {}\n\nAutonomous changes by OpenShark coding agent.",
        task.lines().next().unwrap_or(task).trim()
    );

    let result = git.execute(&format!("commit {}", commit_msg))?;
    Ok(format!("Auto-commit result:\n{}", result))
}

/// Auto-run tests to verify changes after a successful autonomous run.
async fn auto_run_tests() -> anyhow::Result<String> {
    use crate::tools::test_runner::TestTool;
    use crate::tools::Tool;

    let test_tool = TestTool;
    match test_tool.execute("run") {
        Ok(result) => Ok(format!("Auto-test result:\n{}", result)),
        Err(e) => Err(anyhow::anyhow!("Auto-test failed: {}", e)),
    }
}
pub fn format_ndjson(event: &HeadlessEvent) -> String {
    match serde_json::to_string(event) {
        Ok(json) => json,
        Err(e) => format!("{{\"type\":\"error\",\"message\":\"{}\"}}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headless_config_default() {
        let config = HeadlessConfig::default();
        assert!(!config.yolo);
        assert!(!config.autonomous);
        assert!(!config.json);
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.max_turns, 50);
    }

    #[test]
    fn test_autonomous_mode_flag() {
        let mut config = HeadlessConfig::default();
        config.autonomous = true;
        assert!(config.autonomous);
    }

    #[test]
    fn test_event_serialization() {
        let event = HeadlessEvent::Start {
            task: "test".to_string(),
            model: "gpt-4".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).expect("Headless event serialization should not fail");
        assert!(json.contains("\"type\":\"start\""));
        assert!(json.contains("test"));
    }

    #[test]
    fn test_ndjson_format() {
        let event = HeadlessEvent::Complete {
            summary: "done".to_string(),
            total_turns: 5,
            duration_secs: 10,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = format_ndjson(&event);
        assert!(json.contains("\"type\":\"complete\""));
        assert!(json.contains("5"));
    }

    #[test]
    fn test_system_prompt_includes_tools() {
        let prompt = build_system_prompt(false);
        assert!(prompt.contains("Available Tools"));
        assert!(prompt.contains("edit"));
        assert!(prompt.contains("search"));
        assert!(prompt.contains("terminal"));
    }

    #[test]
    fn test_system_prompt_autonomous() {
        let prompt = build_system_prompt(true);
        assert!(prompt.contains("AUTONOMOUS MODE"));
    }

    #[test]
    fn test_system_prompt_guarded() {
        let prompt = build_system_prompt(false);
        assert!(prompt.contains("GUARDED MODE"));
    }
}
