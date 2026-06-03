//! Self-Correction Loop — Retry failed tool calls with AI guidance.
//!
//! When a tool fails, the agent sends the error back to the model
//! Self-Correction Loop — Retry failed tools with AI guidance

#![allow(dead_code)]

use crate::providers::{Provider, ChatRequest, Message};
use anyhow::Result;

/// Configuration for self-correction behavior.
#[derive(Debug, Clone)]
pub struct SelfCorrectionConfig {
    pub max_retries: usize,
    pub enabled: bool,
}

impl Default for SelfCorrectionConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            enabled: true,
        }
    }
}

/// Result of a self-correction attempt.
#[derive(Debug, Clone)]
pub struct CorrectionResult {
    pub success: bool,
    pub final_output: String,
    pub attempts: usize,
    pub resolved: bool,
}

/// Run a tool with self-correction: on failure, ask the model to fix it.
pub async fn run_with_correction<F, Fut>(
    provider: &Provider,
    model: &str,
    system_prompt: &str,
    task_description: &str,
    mut attempt_fn: F,
    config: &SelfCorrectionConfig,
) -> Result<CorrectionResult>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    if !config.enabled {
        let output = attempt_fn().await?;
        return Ok(CorrectionResult {
            success: true,
            final_output: output,
            attempts: 1,
            resolved: true,
        });
    }

    let mut history: Vec<Message> = vec![
        Message {
            role: "system".to_string(),
            content: system_prompt.to_string(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        },
    ];

    for attempt in 1..=config.max_retries {
        match attempt_fn().await {
            Ok(output) => {
                return Ok(CorrectionResult {
                    success: true,
                    final_output: output,
                    attempts: attempt,
                    resolved: true,
                });
            }
            Err(e) => {
                let error_msg = format!(
                    "Attempt {}/{} failed: {}. Please analyze the error and provide a corrected approach.",
                    attempt, config.max_retries, e
                );
                history.push(Message {
                    role: "user".to_string(),
                    content: if attempt == 1 {
                        format!("{}\n\n{}", task_description, error_msg)
                    } else {
                        error_msg
                    },
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                });

                if attempt >= config.max_retries {
                    return Ok(CorrectionResult {
                        success: false,
                        final_output: e.to_string(),
                        attempts: attempt,
                        resolved: false,
                    });
                }

                // Ask model for correction guidance
                let request = ChatRequest::new(model.to_string(), history.clone(), false);
                match provider.chat(request).await {
                    Ok(response) => {
                        let guidance = response
                            .choices
                            .first()
                            .map(|c| c.message.content.clone())
                            .unwrap_or_else(|| "No guidance provided.".to_string());
                        history.push(Message {
                            role: "assistant".to_string(),
                            content: guidance,
                            images: None,
                            tool_call_id: None,
                            tool_calls: None,
                            reasoning_content: None,
                        });
                    }
                    Err(_) => {
                        // If model call fails, just retry blindly
                    }
                }
            }
        }
    }

    unreachable!()
}
