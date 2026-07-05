//! OpenShark AI Harness — Response Types
//!
//! Defines the output types for the harness engine.

use crate::providers::StreamMetrics;

/// Result of executing a single tool call.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolExecutionResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: String,
    pub result: String,
    pub success: bool,
    pub execution_time_ms: u64,
}

/// A response from a single model.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ModelResponse {
    pub model_name: String,
    pub provider_name: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<crate::providers::ToolCallRequest>,
    pub metrics: StreamMetrics,
    pub finish_reason: Option<String>,
}

/// The complete harness response after one turn.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HarnessResponse {
    /// Primary model response (drives the conversation).
    pub primary: ModelResponse,
    /// Secondary model responses (for comparison).
    pub secondary: Vec<ModelResponse>,
    /// Tool execution results (if any tool calls were made).
    pub tool_results: Vec<ToolExecutionResult>,
    /// Whether the turn required tool execution.
    pub had_tool_calls: bool,
    /// Total tokens used across all models.
    pub total_tokens: u64,
    /// Total estimated cost in USD.
    pub total_cost_usd: f64,
}

/// State of an ongoing harness conversation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HarnessState {
    pub session_id: String,
    pub messages: Vec<crate::providers::Message>,
    pub tool_results_history: Vec<ToolExecutionResult>,
    pub turn_count: usize,
    pub total_tokens: u64,
    pub total_cost_usd: f64,
}

impl HarnessState {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            messages: Vec::new(),
            tool_results_history: Vec::new(),
            turn_count: 0,
            total_tokens: 0,
            total_cost_usd: 0.0,
        }
    }
}
