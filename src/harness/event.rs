//! OpenShark AI Harness — Streaming Events
//!
//! Events emitted by the harness engine during a streaming turn. These are
//! converted by the TUI into its own `StreamEvent` types.

use crate::providers::{StreamMetrics, ToolCallRequest};

/// Events sent from the harness engine during `run_turn_streaming`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum HarnessEvent {
    /// Streaming/model work started.
    Start,
    /// A content chunk arrived (for true streaming UIs).
    Chunk(String),
    /// A reasoning/thinking chunk arrived.
    ReasoningChunk(String),
    /// The model emitted a native tool call.
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// A tool was executed and returned a result.
    ToolResult {
        tool_call_id: String,
        name: String,
        args: String,
        result: String,
        success: bool,
    },
    /// The assistant's initial response (with or without tool calls) is complete.
    AssistantComplete {
        content: String,
        reasoning: Option<String>,
        tool_calls: Vec<ToolCallRequest>,
        metrics: StreamMetrics,
        finish_reason: Option<String>,
    },
    /// The final assistant response after any tool execution loops.
    FollowUp(String),
    /// A multi-model secondary response.
    MultiModelResponse {
        name: String,
        content: String,
        metrics: StreamMetrics,
    },
    /// An error occurred.
    Error(String),
    /// A system/info message (e.g. auto-execution notice).
    SystemMessage(String),
    /// Streaming finished.
    Done,
}
