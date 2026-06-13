use std::collections::HashMap;
use std::time::Instant;

use crate::memory::ToolCall;
use crate::providers::{Message, StreamMetrics};
use crate::tools::ToolSuggestion;
use chrono::Utc;
use uuid::Uuid;

use super::{App, AppMode};

/// Events sent from the background streaming task to the main TUI loop.
#[derive(Debug)]
pub(crate) enum StreamEvent {
    /// Streaming started — show the user that we're waiting.
    Start,
    /// A content chunk arrived (for true streaming UIs).
    Chunk(String),
    /// A reasoning/thinking chunk arrived (shown in real-time before response).
    ReasoningChunk(String),
    /// The full response is complete.
    ResponseComplete {
        content: String,
        metrics: StreamMetrics,
    },
    /// A tool was invoked and returned a result.
    ToolResult {
        name: String,
        args: String,
        result: String,
        success: bool,
    },
    /// Follow-up assistant message after tool execution.
    FollowUp(String),
    /// Multi-model secondary response.
    MultiModelResponse {
        name: String,
        content: String,
        metrics: StreamMetrics,
    },
    /// Switch to tool-approval mode.
    #[allow(dead_code)]
    SetPendingSuggestion(ToolSuggestion),
    /// Multi-file edit batch pending approval.
    SetPendingBatch(crate::tools::ToolBatch),
    /// An error occurred.
    Error(String),
    /// A system/info message (e.g. auto-execution notice).
    SystemMessage(String),
    /// Streaming finished (success or error).
    Done,
    /// Batched tool results from multi-tool execution (collapsed display).
    ToolResultsBatch { results: Vec<ToolResultEntry> },
}

/// A single tool result for batched display.
#[derive(Debug, Clone)]
pub(crate) struct ToolResultEntry {
    pub name: String,
    pub args: String,
    pub result: String,
    pub success: bool,
}

/// A secondary model response attached to a primary assistant message.
#[derive(Debug, Clone)]
pub(crate) struct SecondaryResponse {
    pub model_name: String,
    pub content: String,
    pub latency_ms: u64,
    pub tokens: u32,
}

/// Per-agent streaming state for swarm mode.
#[derive(Debug, Clone)]
pub(crate) struct AgentStreamState {
    pub agent_id: String,
    pub agent_name: String,
    pub role: String,
    pub content: String,
    pub is_streaming: bool,
    pub tool_results: Vec<(String, String, bool)>, // (tool_name, result, success)
}

pub(crate) fn apply_stream_event(app: &mut App, event: StreamEvent) {
    use super::{
        detect_high_confidence_suggestion, generate_edit_diff, parse_embedded_tools,
        stream_model_response_task, strip_think_tags, strip_tool_lines,
    };
    match event {
        StreamEvent::Start => {
            app.is_streaming = true;
            app.streaming_content.clear();
            app.reasoning_content.clear();
            app.is_reasoning = false;
            app.stream_start_time = Some(Instant::now());
        }
        StreamEvent::Chunk(chunk) => {
            let cleaned = strip_think_tags(&chunk);
            app.streaming_content.push_str(&strip_tool_lines(&cleaned));
        }
        StreamEvent::ReasoningChunk(chunk) => {
            let cleaned = strip_tool_lines(&chunk);
            app.reasoning_content.push_str(&cleaned);
            app.is_reasoning = true;
        }
        StreamEvent::ResponseComplete { content, metrics } => {
            app.is_streaming = false;
            app.stream_start_time = None;
            app.session_perf.record_response(&metrics);
            let _ = app.memory.save_performance_metric(
                "first_token",
                &app.model,
                metrics.first_token_latency_ms,
                Some(&format!("cached={}", metrics.cached)),
            );
            let _ = app.memory.save_performance_metric(
                "total_latency",
                &app.model,
                metrics.total_latency_ms,
                Some(&format!("tokens={}", metrics.tokens_generated)),
            );

            let reasoning_to_save = Some(app.reasoning_content.clone());

            let embedded_tools = parse_embedded_tools(&content);
            if !embedded_tools.is_empty() {
                let display_content = strip_think_tags(&strip_tool_lines(&content));
                if !display_content.trim().is_empty() {
                    app.add_assistant_message(display_content, reasoning_to_save);
                } else {
                    if let Some(ref r) = reasoning_to_save {
                        app.add_assistant_message("".to_string(), Some(r.clone()));
                    }
                }
                for (tool_name, args) in &embedded_tools {
                    app.model_messages.push(Message {
                        role: "assistant".to_string(),
                        content: format!("TOOL:{} {}", tool_name, args),
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                    });
                }
            } else if content.starts_with("TOOL:") || content.starts_with("TOOL.") {
                let rest = &content[5..];
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if !parts.is_empty() {
                    let tool_name = parts[0].trim().to_string();
                    let args = parts.get(1).unwrap_or(&"").trim().to_string();
                    app.add_assistant_message(
                        format!("🔧 Using tool: {} {}", tool_name, args),
                        reasoning_to_save.clone(),
                    );
                    app.model_messages.push(Message {
                        role: "assistant".to_string(),
                        content: format!("TOOL:{} {}", tool_name, args),
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                    });
                }
            } else {
                let clean_content = strip_think_tags(&content);
                app.add_assistant_message(clean_content, reasoning_to_save);
                if let Some(suggestion) = detect_high_confidence_suggestion(&content) {
                    match app
                        .security_engine
                        .check_tool_call(&suggestion.tool_name, &suggestion.args)
                    {
                        crate::security::SecurityDecision::RequireApproval {
                            reason: _,
                            risk_level,
                        } => {
                            let tool_name = suggestion.tool_name.clone();
                            app.pending_suggestion = Some(suggestion);
                            app.mode = AppMode::ToolApproval;
                            app.tool_approval_shown_at = Some(Instant::now());
                            app.add_system_message(format!(
                                "🔒 Tool '{}' requires approval (risk: {:?}) — press y/n",
                                tool_name, risk_level
                            ));
                        }
                        crate::security::SecurityDecision::Deny { reason } => {
                            app.add_system_message(format!(
                                "🚫 Tool '{}' blocked: {}",
                                suggestion.tool_name, reason
                            ));
                        }
                        crate::security::SecurityDecision::Allow => {}
                    }
                }
            }
        }
        StreamEvent::ToolResultsBatch { results } => {
            let mut groups: HashMap<String, (usize, usize)> = HashMap::new();
            for r in &results {
                let entry = groups.entry(r.name.clone()).or_insert((0, 0));
                if r.success {
                    entry.0 += 1;
                } else {
                    entry.1 += 1;
                }
            }
            let total = results.len();
            let mut summary = format!("📊 Tool results ({} total):\n", total);
            let mut names: Vec<&String> = groups.keys().collect();
            names.sort();
            for name in names {
                let (ok, err) = groups[name];
                if ok > 0 && err == 0 {
                    summary.push_str(&format!("  ✅ {} × {}\n", name, ok));
                } else if ok > 0 && err > 0 {
                    summary.push_str(&format!("  ⚠️ {}: {} ok, {} failed\n", name, ok, err));
                } else {
                    summary.push_str(&format!("  ❌ {}: {} failed\n", name, err));
                }
            }
            app.add_system_message(summary);

            for r in &results {
                let tool_call = ToolCall {
                    id: Uuid::new_v4().to_string(),
                    session_id: app.session_id.clone(),
                    tool_name: r.name.clone(),
                    args: r.args.clone(),
                    result: r.result.clone(),
                    success: r.success,
                    created_at: Utc::now(),
                };
                let _ = app.memory.save_tool_call(&tool_call);
                if r.success {
                    app.tool_calls_count += 1;
                }
                if let Some(ref evolution) = app.evolution {
                    evolution.track_tool_outcome(&r.name, r.success, 0);
                }
                app.model_messages.push(Message {
                    role: "user".to_string(),
                    content: format!("Tool result: {}", r.result),
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                });
            }
        }
        StreamEvent::ToolResult {
            name,
            args,
            result,
            success,
        } => {
            let display = if success {
                format!("Result: {}", &result[..result.len().min(200)])
            } else {
                format!("Tool execution failed: {}", result)
            };
            app.add_system_message(display);

            let tool_call = ToolCall {
                id: Uuid::new_v4().to_string(),
                session_id: app.session_id.clone(),
                tool_name: name.clone(),
                args: args.clone(),
                result: result.clone(),
                success,
                created_at: Utc::now(),
            };
            let _ = app.memory.save_tool_call(&tool_call);
            if success {
                app.tool_calls_count += 1;
            }

            if let Some(ref evolution) = app.evolution {
                evolution.track_tool_outcome(&name, success, 0);
            }

            app.model_messages.push(Message {
                role: "user".to_string(),
                content: format!("Tool result: {}", result),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
        }
        StreamEvent::FollowUp(content) => {
            // FollowUp is the SYNTHESIS phase — the model should NOT output more tools.
            // If it does, strip them and display as text instead of re-executing.
            let embedded_tools = parse_embedded_tools(&content);
            if !embedded_tools.is_empty() {
                // Model incorrectly output tools during synthesis phase.
                // Strip tool lines and display the remaining text as the assistant's response.
                let display_content = strip_think_tags(&strip_tool_lines(&content));
                if !display_content.trim().is_empty() {
                    app.add_system_message(
                        "⚠️ Model tried to call more tools during synthesis. Stripped tool calls."
                            .to_string(),
                    );
                    app.add_assistant_message(display_content, None);
                } else {
                    app.add_system_message(
                        "⚠️ Model output only tool calls during synthesis phase (ignored)."
                            .to_string(),
                    );
                }
                // Do NOT re-execute tools — that causes infinite loops
                return;
            }

            let trimmed = content.trim();
            if trimmed.is_empty() {
                app.empty_response_count += 1;
                if app.empty_response_count >= 1 {
                    app.add_system_message(
                        "⚠️ Model returned empty synthesis. Showing raw tool results:".to_string(),
                    );
                    let mut found_results = Vec::new();
                    for msg in app.model_messages.iter().rev().take(20) {
                        if msg.role == "user" && msg.content.starts_with("Tool result:") {
                            found_results.push(msg.content.clone());
                        }
                    }
                    if found_results.is_empty() {
                        app.add_system_message("No recent tool results to display.".to_string());
                    } else {
                        for result in found_results.iter().rev() {
                            app.add_system_message(result.clone());
                        }
                    }
                    app.empty_response_count = 0;
                } else {
                    app.add_system_message(
                        "⚠️ Response was empty — re-prompting for synthesis...".to_string(),
                    );
                    app.model_messages.push(Message {
                        role: "assistant".to_string(),
                        content: content.clone(),
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                    });
                    app.model_messages.push(Message {
                        role: "user".to_string(),
                        content: "Provide a COMPLETE synthesis of the tool results. Explain what was found, what it means, and the next step.".to_string(),
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                    });
                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    app.stream_rx = Some(rx);
                    let provider = app.provider.clone();
                    let model = app.model.clone();
                    let model_config = app.model_config.clone();
                    let model_messages = app.model_messages.clone();
                    let is_multi_model = app.multi_model_mode;
                    let config = app.config.clone();
                    tokio::spawn(async move {
                        let _ = stream_model_response_task(
                            tx,
                            provider,
                            model,
                            model_config,
                            model_messages,
                            is_multi_model,
                            config,
                        )
                        .await;
                    });
                }
            } else {
                app.empty_response_count = 0;
                app.add_assistant_message(strip_think_tags(&content), None);
            }
        }
        StreamEvent::MultiModelResponse {
            name,
            content,
            metrics,
        } => {
            if !content.is_empty()
                && let Some(last_idx) = app.messages.iter().rposition(|m| m.role == "assistant")
            {
                app.messages[last_idx]
                    .multi_model_responses
                    .push(SecondaryResponse {
                        model_name: name,
                        content,
                        latency_ms: metrics.total_latency_ms,
                        tokens: metrics.tokens_generated,
                    });
            }
        }
        StreamEvent::SetPendingBatch(batch) => {
            app.pending_batch = Some(batch);
            app.batch_selected = 0;
            app.add_system_message(
                "🔧 Multi-file edit batch received. Use /approve or /reject to handle.".to_string(),
            );
        }
        StreamEvent::SetPendingSuggestion(suggestion) => {
            if suggestion.tool_name == "edit"
                && let Some(diff) = generate_edit_diff(&suggestion.args)
            {
                app.pending_diff = Some(diff);
                app.pending_suggestion = Some(suggestion);
                app.mode = AppMode::DiffPreview;
                app.diff_scroll = 0;
                return;
            }
            app.pending_suggestion = Some(suggestion);
            app.mode = AppMode::ToolApproval;
            app.tool_approval_shown_at = Some(Instant::now());
        }
        StreamEvent::Error(msg) => {
            app.is_streaming = false;
            app.stream_start_time = None;
            app.add_system_message(msg);
        }
        StreamEvent::SystemMessage(msg) => {
            if msg.starts_with("🔧 Tool") {
                if let Some(idx) = app.last_progress_msg_idx
                    && idx < app.messages.len()
                {
                    app.messages[idx].content = msg;
                    return;
                }
                app.add_system_message(msg);
                app.last_progress_msg_idx = Some(app.messages.len() - 1);
            } else {
                app.last_progress_msg_idx = None;
                app.add_system_message(msg);
            }
        }
        StreamEvent::Done => {
            app.is_streaming = false;
            app.stream_start_time = None;
        }
    }
}
