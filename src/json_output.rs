//! JSON / NDJSON Output Mode for OpenShark
//!
//! Structured output for piping into other tools.
//! JSON Output Mode — Structured NDJSON output for headless/CI usage

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputEvent {
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
    Message {
        role: String,
        content: String,
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

/// Emit a single NDJSON event to stdout.
pub fn emit_event(event: &OutputEvent) {
    match serde_json::to_string(event) {
        Ok(json) => {
            let _ = writeln!(std::io::stdout(), "{}", json);
        }
        Err(e) => {
            let fallback = format!(
                "{{\"type\":\"error\",\"message\":\"serialization failed: {}\"}}",
                e
            );
            let _ = writeln!(std::io::stdout(), "{}", fallback);
        }
    }
}

/// Emit a tool call event.
pub fn emit_tool_call(name: &str, args: &str, turn: usize) {
    emit_event(&OutputEvent::ToolCall {
        name: name.to_string(),
        args: args.to_string(),
        turn,
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}

/// Emit a tool result event.
pub fn emit_tool_result(name: &str, output: &str, success: bool, turn: usize) {
    emit_event(&OutputEvent::ToolResult {
        name: name.to_string(),
        output: output.to_string(),
        success,
        turn,
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}

/// Emit a message event.
pub fn emit_message(role: &str, content: &str, turn: usize) {
    emit_event(&OutputEvent::Message {
        role: role.to_string(),
        content: content.to_string(),
        turn,
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}

/// Emit an error event.
pub fn emit_error(message: &str, turn: usize) {
    emit_event(&OutputEvent::Error {
        message: message.to_string(),
        turn,
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}

/// Emit a completion event.
pub fn emit_complete(summary: &str, turns: usize, duration_secs: u64) {
    emit_event(&OutputEvent::Complete {
        summary: summary.to_string(),
        total_turns: turns,
        duration_secs,
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}

/// Enable NDJSON output mode (disables TUI, enables structured logging).
pub fn enable_json_mode() {
    // In headless mode, this is implicit. This function serves as a marker.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = OutputEvent::Complete {
            summary: "done".to_string(),
            total_turns: 5,
            duration_secs: 10,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"complete\""));
        assert!(json.contains("5"));
    }

    #[test]
    fn test_emit_message_format() {
        let event = OutputEvent::Message {
            role: "assistant".to_string(),
            content: "hello".to_string(),
            turn: 1,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("assistant"));
        assert!(json.contains("hello"));
    }
}
