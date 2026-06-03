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

/// Run a task in headless mode.
/// Returns the final summary string.
pub async fn run_headless(
    config: HeadlessConfig,
    provider: crate::providers::Provider,
    model: String,
    event_tx: Option<mpsc::UnboundedSender<HeadlessEvent>>,
) -> anyhow::Result<String> {
    let start = Instant::now();
    let mut turn = 0usize;

    let now = || chrono::Utc::now().to_rfc3339();

    if let Some(tx) = &event_tx {
        let _ = tx.send(HeadlessEvent::Start {
            task: config.task.clone(),
            model: model.clone(),
            timestamp: now(),
        });
    }

    let mut current_prompt = config.task.clone();
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
            messages: vec![
                crate::providers::Message {
                    role: "user".to_string(),
                    content: current_prompt.clone(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
            ],
            stream: false,
            max_tokens: None,
            temperature: None,
            tools: None,
        };

        let response = match provider.chat(request).await {
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
                content: response.clone(),
                turn,
                timestamp: now(),
            });
        }

        if response.contains("TASK_COMPLETE")
            || response.contains("Done!")
            || response.contains("All done")
        {
            summary = response;
            break;
        }

        current_prompt = format!("Continue working on the task. Previous response: {}", response);
        summary.push_str(&response);
        summary.push('\n');
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
}
