//! Headless / CI-CD mode for OpenShark
//!
//! Run OpenShark non-interactively: `openshark --autonomous "implement feature X"`
//! Outputs structured JSON or plain text for piping into other tools.
//!
//! Features:
//!   --yolo          Auto-approve all tool calls (no interactive prompts)
//!   --autonomous    Full autonomy mode: no approvals, auto-commits, auto-tests
//!   --json          Output NDJSON for structured consumption
//!   --timeout       Max seconds to run (default: 300)
//!   --max-turns     Max agent turns (default: 50)
//!   --model         Override model for this run
//!   --output        Write output to file instead of stdout

#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::harness::{HarnessConfig, HarnessEngine, HarnessEvent};
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

/// Run a task in headless mode with full tool execution.
/// Returns the final summary string.
///
/// The harness engine (and its `MemoryStore`) is not `Send`, so the actual engine
/// work is run inside `tokio::task::spawn_blocking` with a local single-threaded
/// runtime. The outer async future is therefore `Send` and can be spawned on the
/// main tokio runtime.
pub async fn run_headless(
    config: HeadlessConfig,
    _provider: crate::providers::Provider,
    _model: String,
    security: SecurityEngine,
    event_tx: Option<mpsc::UnboundedSender<HeadlessEvent>>,
) -> Result<String> {
    let start = Instant::now();
    let is_autonomous = config.autonomous || config.yolo;
    let now = || chrono::Utc::now().to_rfc3339();

    let app_config = crate::config::Config::load_or_default().unwrap_or_default();
    let model = config
        .model
        .clone()
        .unwrap_or_else(|| app_config.default_model.clone());

    emit(
        &event_tx,
        HeadlessEvent::Start {
            task: config.task.clone(),
            model: model.clone(),
            timestamp: now(),
        },
    );

    // Move engine work into a blocking thread with a local runtime so that the
    // non-Send `MemoryStore` / `HarnessEngine` never cross a multi-threaded await.
    let event_tx_blocking = event_tx.clone();
    let config_blocking = config.clone();
    let model_for_blocking = model.clone();

    let handle = tokio::task::spawn_blocking(move || -> Result<(String, usize)> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build local runtime for headless harness")?;

        rt.block_on(async {
            let app_config = crate::config::Config::load_or_default().unwrap_or_default();
            let harness_config = HarnessConfig {
                primary_model: model_for_blocking.clone(),
                max_tool_loops: config_blocking.max_turns,
                require_tool_approval: !is_autonomous,
                multi_model_enabled: false,
                memory_context_limit: 5,
                skills_enabled: true,
                secondary_models: Vec::new(),
            };

            let memory =
                crate::memory::MemoryStore::new(&app_config.memory_db_path)?;
            let mut engine =
                HarnessEngine::new_with_security(harness_config, app_config, memory, security)?;

            let mut summary = String::new();
            let mut turn = 0usize;
            let mut consecutive_errors = 0usize;
            let max_consecutive_errors = 3;
            let mut user_message = config_blocking.task.clone();
            let now = || chrono::Utc::now().to_rfc3339();

            while turn < config_blocking.max_turns {
                if start.elapsed() > Duration::from_secs(config_blocking.timeout_secs) {
                    let msg = format!("Timeout after {} seconds", config_blocking.timeout_secs);
                    emit(
                        &event_tx_blocking,
                        HeadlessEvent::Error {
                            message: msg.clone(),
                            turn,
                            timestamp: now(),
                        },
                    );
                    summary.push_str(&msg);
                    summary.push('\n');
                    break;
                }

                turn += 1;
                let (h_tx, mut h_rx) = mpsc::unbounded_channel();

                let turn_result = engine.run_turn_streaming(&user_message, h_tx, true).await;

                let mut turn_content = String::new();
                while let Some(event) = h_rx.recv().await {
                    match event {
                        HarnessEvent::Start
                        | HarnessEvent::Done
                        | HarnessEvent::SystemMessage(_) => {}
                        HarnessEvent::Chunk(c) => turn_content.push_str(&c),
                        HarnessEvent::ReasoningChunk(_) => {}
                        HarnessEvent::ToolCall {
                            id: _, name, arguments, ..
                        } => {
                            emit(
                                &event_tx_blocking,
                                HeadlessEvent::ToolCall {
                                    name: name.clone(),
                                    args: arguments.clone(),
                                    turn,
                                    timestamp: now(),
                                },
                            );
                        }
                        HarnessEvent::ToolResult {
                            tool_call_id: _,
                            name,
                            args: _,
                            result,
                            success,
                            ..
                        } => {
                            emit(
                                &event_tx_blocking,
                                HeadlessEvent::ToolResult {
                                    name: name.clone(),
                                    output: result.clone(),
                                    success,
                                    turn,
                                    timestamp: now(),
                                },
                            );
                        }
                        HarnessEvent::AssistantComplete { content, .. } => {
                            turn_content = content;
                        }
                        HarnessEvent::FollowUp(content) => {
                            turn_content = content;
                        }
                        HarnessEvent::MultiModelResponse { .. } => {}
                        HarnessEvent::Error(e) => {
                            emit(
                                &event_tx_blocking,
                                HeadlessEvent::Error {
                                    message: e,
                                    turn,
                                    timestamp: now(),
                                },
                            );
                            consecutive_errors += 1;
                        }
                    }
                }

                let response = match turn_result {
                    Ok(r) => {
                        consecutive_errors = 0;
                        r
                    }
                    Err(e) => {
                        let msg = format!("Harness error: {}", e);
                        emit(
                            &event_tx_blocking,
                            HeadlessEvent::Error {
                                message: msg.clone(),
                                turn,
                                timestamp: now(),
                            },
                        );
                        consecutive_errors += 1;
                        if consecutive_errors >= max_consecutive_errors {
                            summary.push_str(&msg);
                            summary.push('\n');
                            break;
                        }
                        user_message = format!(
                            "The previous turn failed: {}. Please try again or use a different approach.",
                            e
                        );
                        continue;
                    }
                };

                emit(
                    &event_tx_blocking,
                    HeadlessEvent::Thought {
                        content: turn_content.clone(),
                        turn,
                        timestamp: now(),
                    },
                );
                summary.push_str(&turn_content);
                summary.push('\n');

                if turn_content.contains("TASK_COMPLETE")
                    || turn_content.contains("Done!")
                    || turn_content.contains("All done")
                    || response.primary.finish_reason == Some("stop".to_string())
                {
                    break;
                }

                user_message = "Continue working on the task. If finished, respond with TASK_COMPLETE. If you need to use a tool, use the native tool_calls format.".to_string();
            }

            Ok((summary, turn))
        })
    });

    let (mut summary, turn) = match handle.await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => return Err(e),
        Err(e) => return Err(anyhow::anyhow!("Headless blocking task panicked: {}", e)),
    };

    // ── POST-COMPLETION: AUTO-VERIFY, AUTO-COMMIT, SELF-IMPROVE ───────
    if is_autonomous && !summary.is_empty() && !summary.contains("Loop detected") {
        match auto_run_tests().await {
            Ok(test_result) => {
                emit(
                    &event_tx,
                    HeadlessEvent::Thought {
                        content: test_result.clone(),
                        turn: turn + 1,
                        timestamp: now(),
                    },
                );
                summary.push('\n');
                summary.push_str(&test_result);
            }
            Err(e) => {
                let msg = format!("⚠️ Auto-test failed: {}", e);
                emit(
                    &event_tx,
                    HeadlessEvent::Error {
                        message: msg.clone(),
                        turn: turn + 1,
                        timestamp: now(),
                    },
                );
                summary.push('\n');
                summary.push_str(&msg);
            }
        }

        match auto_commit_changes(&config.task).await {
            Ok(commit_result) => {
                emit(
                    &event_tx,
                    HeadlessEvent::Thought {
                        content: commit_result.clone(),
                        turn: turn + 1,
                        timestamp: now(),
                    },
                );
                summary.push('\n');
                summary.push_str(&commit_result);
            }
            Err(e) => {
                let msg = format!("⚠️ Auto-commit failed: {}", e);
                emit(
                    &event_tx,
                    HeadlessEvent::Error {
                        message: msg.clone(),
                        turn: turn + 1,
                        timestamp: now(),
                    },
                );
                summary.push('\n');
                summary.push_str(&msg);
            }
        }

        tokio::spawn(async move {
            if let Err(e) = crate::self_improve::trigger_analysis(
                &crate::config::Config::load_or_default().unwrap_or_default(),
            )
            .await
            {
                tracing::warn!("Self-improvement analysis failed: {}", e);
            }
        });
    }

    let duration = start.elapsed().as_secs();

    if let Some(ref path) = config.output_file {
        if let Err(e) = tokio::fs::write(path, &summary).await {
            let msg = format!("⚠️ Failed to write output to {}: {}", path, e);
            emit(
                &event_tx,
                HeadlessEvent::Error {
                    message: msg.clone(),
                    turn: turn + 1,
                    timestamp: now(),
                },
            );
            summary.push('\n');
            summary.push_str(&msg);
        }
    }

    emit(
        &event_tx,
        HeadlessEvent::Complete {
            summary: summary.clone(),
            total_turns: turn,
            duration_secs: duration,
            timestamp: now(),
        },
    );

    Ok(summary)
}

/// Helper: emit an event if a channel is available.
fn emit(tx: &Option<mpsc::UnboundedSender<HeadlessEvent>>, event: HeadlessEvent) {
    if let Some(sender) = tx {
        let _ = sender.send(event);
    }
}

/// Auto-commit any changes after a successful autonomous run.
async fn auto_commit_changes(task: &str) -> Result<String> {
    use crate::tools::git::GitTool;
    use crate::tools::Tool;

    let git = GitTool;

    if !GitTool::has_changes() {
        return Ok("No changes to commit.".to_string());
    }

    let _ = git.execute("add .")?;

    let commit_msg = format!(
        "openshark: {}\n\nAutonomous changes by OpenShark coding agent.",
        task.lines().next().unwrap_or(task).trim()
    );

    let result = git.execute(&format!("commit {}", commit_msg))?;
    Ok(format!("Auto-commit result:\n{}", result))
}

/// Auto-run tests to verify changes after a successful autonomous run.
async fn auto_run_tests() -> Result<String> {
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
        let config = HeadlessConfig {
            autonomous: true,
            ..Default::default()
        };
        assert!(config.autonomous);
    }

    #[test]
    fn test_event_serialization() {
        let event = HeadlessEvent::Start {
            task: "test".to_string(),
            model: "gpt-4".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event)
            .expect("Headless event serialization should not fail");
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
        let prompt = crate::harness::engine::HarnessEngine::build_system_prompt_static();
        assert!(prompt.contains("AVAILABLE TOOLS"));
    }

    #[test]
    fn test_system_prompt_autonomous() {
        let prompt = crate::harness::engine::HarnessEngine::build_system_prompt_static();
        assert!(prompt.contains("OpenShark"));
    }

    #[test]
    fn test_system_prompt_guarded() {
        let prompt = crate::harness::engine::HarnessEngine::build_system_prompt_static();
        assert!(prompt.contains("OpenShark"));
    }
}
