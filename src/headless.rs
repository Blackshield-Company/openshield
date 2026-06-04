//! Headless / CI-CD mode for OpenShark
//!
//! Run OpenShark non-interactively: `openshark --yolo "implement feature X"`
//! Outputs structured JSON or plain text for piping into other tools.
//!
//! Features:
//!   --yolo          Auto-approve all tool calls (no interactive prompts)
//!   --json          Output NDJSON for structured consumption
//!   --timeout       Max seconds to run (default: 300)
//!   --max-turns     Max agent turns (default: 50)
//!   --model         Override model for this run
//!   --output        Write output to file instead of stdout

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::tools::{detect_tool_suggestions, find_tool};

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
            json: false,
            timeout_secs: 300,
            max_turns: 50,
            model: None,
            output_file: None,
        }
    }
}

/// Build the system prompt for headless mode with tool descriptions.
fn build_system_prompt() -> String {
    let tool_descs = crate::tools::get_all_tool_descriptions();
    let tool_list: Vec<String> = tool_descs
        .iter()
        .map(|(name, desc)| format!("  - {}: {}", name, desc))
        .collect();

    format!(
        "You are OpenShark, an autonomous AI coding agent running in headless mode.\n\
         Your task will be given as a user message. Execute it by using tools.\n\n\
         ## Available Tools\n\
         You can invoke tools using any of these formats:\n\
         - `TOOL:tool_name args` (preferred, highest confidence)\n\
         - ```tool:tool_name\\nargs\\n```\n\
         - Natural language: \"I should use search to find...\"\n\n\
         ## Tool Reference\n\
         {}\n\n\
         ## Rules\n\
         - Execute tools to accomplish the task. Do not just describe what you would do.\n\
         - After each tool result, decide what to do next.\n\
         - When the task is complete, respond with TASK_COMPLETE on its own line.\n\
         - If you encounter an error, try to fix it. If unfixable, explain and respond TASK_COMPLETE.\n\
         - Be concise. Focus on the task.\n\
         - You can use multiple tools per turn by listing multiple TOOL: lines.",
        tool_list.join("\n")
    )
}

/// Run a task in headless mode with full tool execution.
/// Returns the final summary string.
pub async fn run_headless(
    config: HeadlessConfig,
    provider: crate::providers::Provider,
    model: String,
    event_tx: Option<mpsc::UnboundedSender<HeadlessEvent>>,
) -> anyhow::Result<String> {
    let start = Instant::now();
    let mut turn = 0usize;
    let security_engine = crate::security::SecurityEngine::new(
        crate::security::SecurityConfig::default(),
    )
    .unwrap_or_else(|_| {
        // Fall back to a default engine if config fails
        crate::security::SecurityEngine::new(
            crate::security::SecurityConfig::default(),
        ).expect("Failed to create security engine")
    });

    let now = || chrono::Utc::now().to_rfc3339();

    if let Some(tx) = &event_tx {
        let _ = tx.send(HeadlessEvent::Start {
            task: config.task.clone(),
            model: model.clone(),
            timestamp: now(),
        });
    }

    // Build conversation history with system prompt
    let system_prompt = build_system_prompt();
    let mut messages: Vec<crate::providers::Message> = vec![
        crate::providers::Message {
            role: "system".to_string(),
            content: system_prompt,
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        },
        crate::providers::Message {
            role: "user".to_string(),
            content: config.task.clone(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        },
    ];

    let mut summary = String::new();

    while turn < config.max_turns {
        if start.elapsed() > Duration::from_secs(config.timeout_secs) {
            let msg = format!("Timeout after {} seconds", config.timeout_secs);
            if let Some(tx) = &event_tx {
                let _ = tx.send(HeadlessEvent::Error {
                    message: msg.clone(),
                    turn,
                    timestamp: now(),
                });
            }
            summary = msg;
            break;
        }

        turn += 1;

        let request = crate::providers::ChatRequest {
            model: model.clone(),
            messages: messages.clone(),
            stream: false,
            max_tokens: None,
            temperature: None,
            tools: None,
        };

        let response_text = match provider.chat(request).await {
            Ok(r) => r.choices.first().map(|c| c.message.content.clone()).unwrap_or_default(),
            Err(e) => {
                let msg = format!("Provider error: {}", e);
                if let Some(tx) = &event_tx {
                    let _ = tx.send(HeadlessEvent::Error {
                        message: msg.clone(),
                        turn,
                        timestamp: now(),
                    });
                }
                summary = msg;
                break;
            }
        };

        if let Some(tx) = &event_tx {
            let _ = tx.send(HeadlessEvent::Thought {
                content: response_text.clone(),
                turn,
                timestamp: now(),
            });
        }

        // Check for task completion signals
        if response_text.contains("TASK_COMPLETE")
            || response_text.contains("Done!")
            || response_text.contains("All done")
        {
            summary = response_text;
            break;
        }

        // Detect tool calls in the response
        let suggestions = detect_tool_suggestions(&response_text);

        if suggestions.is_empty() {
            // No tools detected — check if the model gave a final answer
            // Push the assistant message and prompt for next step
            messages.push(crate::providers::Message {
                role: "assistant".to_string(),
                content: response_text.clone(),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
            messages.push(crate::providers::Message {
                role: "user".to_string(),
                content: "Continue working on the task. If you are finished, respond with TASK_COMPLETE. If you need to use a tool, use TOOL:tool_name args format.".to_string(),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
            summary.push_str(&response_text);
            summary.push('\n');
            continue;
        }

        // Add assistant message (with tool calls) to history
        messages.push(crate::providers::Message {
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
            if let Some(tx) = &event_tx {
                let _ = tx.send(HeadlessEvent::ToolCall {
                    name: suggestion.tool_name.clone(),
                    args: suggestion.args.clone(),
                    turn,
                    timestamp: now(),
                });
            }

            // Security gate (in yolo mode, auto-approve; otherwise check)
            if !config.yolo {
                match security_engine.check_tool_call(&suggestion.tool_name, &suggestion.args) {
                    crate::security::SecurityDecision::Allow => {}
                    crate::security::SecurityDecision::RequireApproval { reason, .. } => {
                        let msg = format!(
                            "🔒 Tool '{}' requires approval (non-yolo mode): {}",
                            suggestion.tool_name, reason
                        );
                        if let Some(tx) = &event_tx {
                            let _ = tx.send(HeadlessEvent::Error {
                                message: msg.clone(),
                                turn,
                                timestamp: now(),
                            });
                        }
                        tool_results.push_str(&msg);
                        tool_results.push('\n');
                        continue;
                    }
                    crate::security::SecurityDecision::Deny { reason } => {
                        let msg = format!("🚫 Tool '{}' blocked: {}", suggestion.tool_name, reason);
                        if let Some(tx) = &event_tx {
                            let _ = tx.send(HeadlessEvent::Error {
                                message: msg.clone(),
                                turn,
                                timestamp: now(),
                            });
                        }
                        tool_results.push_str(&msg);
                        tool_results.push('\n');
                        continue;
                    }
                }
            }

            // Find and execute the tool
            let result = match find_tool(&suggestion.tool_name) {
                Some(tool) => {
                    match tool.execute(&suggestion.args) {
                        Ok(output) => {
                            let sanitized = security_engine.sanitize_output(
                                &suggestion.tool_name,
                                &output,
                            );
                            if let Some(tx) = &event_tx {
                                let _ = tx.send(HeadlessEvent::ToolResult {
                                    name: suggestion.tool_name.clone(),
                                    output: sanitized.clone(),
                                    success: true,
                                    turn,
                                    timestamp: now(),
                                });
                            }
                            format!("✅ {} ({}): {}", suggestion.tool_name, suggestion.args, sanitized)
                        }
                        Err(e) => {
                            let msg = format!("❌ {} failed: {}", suggestion.tool_name, e);
                            if let Some(tx) = &event_tx {
                                let _ = tx.send(HeadlessEvent::ToolResult {
                                    name: suggestion.tool_name.clone(),
                                    output: msg.clone(),
                                    success: false,
                                    turn,
                                    timestamp: now(),
                                });
                            }
                            msg
                        }
                    }
                }
                None => {
                    let msg = format!("❌ Unknown tool: {}", suggestion.tool_name);
                    if let Some(tx) = &event_tx {
                        let _ = tx.send(HeadlessEvent::Error {
                            message: msg.clone(),
                            turn,
                            timestamp: now(),
                        });
                    }
                    msg
                }
            };

            tool_results.push_str(&result);
            tool_results.push('\n');
        }

        // Feed tool results back as a user message
        messages.push(crate::providers::Message {
            role: "user".to_string(),
            content: format!("Tool results:\n{}", tool_results),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        summary.push_str(&tool_results);
    }

    let duration = start.elapsed().as_secs();

    if let Some(tx) = &event_tx {
        let _ = tx.send(HeadlessEvent::Complete {
            summary: summary.clone(),
            total_turns: turn,
            duration_secs: duration,
            timestamp: now(),
        });
    }

    Ok(summary)
}

/// Format events as NDJSON.
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
        assert!(!config.json);
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.max_turns, 50);
    }

    #[test]
    fn test_event_serialization() {
        let event = HeadlessEvent::Start {
            task: "test".to_string(),
            model: "gpt-4".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
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
        let prompt = build_system_prompt();
        assert!(prompt.contains("Available Tools"));
        assert!(prompt.contains("edit"));
        assert!(prompt.contains("search"));
        assert!(prompt.contains("terminal"));
        assert!(prompt.contains("TOOL:"));
    }
}
