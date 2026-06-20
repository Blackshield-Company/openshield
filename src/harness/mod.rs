//! OpenShark AI Harness Core
//!
//! The unified engine that drives OpenShark's agentic behavior.
//!
//! ## Design
//!
//! The harness is a stateful conversation loop with native tool calling:
//!
//! ```text
//! User Message → [Memory Injection] → [Skill Triggering] → [System Prompt Build]
//!      ↓
//! Model API Call (with tools schema)
//!      ↓
//! Response: content | tool_calls | both
//!      ↓
//! If tool_calls: execute tools → feed results back → loop
//! If content:     display → wait for next user message
//! ```
//!
//! ## Multi-Response Support
//!
//! The harness can query multiple models simultaneously and return all responses
//! for comparison. Primary model drives the tool loop; secondary models provide
//! alternative perspectives.
//!
//! ## Key Features
//!
//! - **Native tool calling**: OpenAI-compatible `tools` + `tool_calls` loop
//! - **Memory integration**: Auto-injects relevant past context from SQLite
//! - **Skill injection**: Triggered skills appended to system prompt
//! - **Multi-model**: Parallel queries to multiple providers
//! - **Streaming**: Real-time content + reasoning display
//! - **Checkpointing**: Auto-save before file edits
//! - **Security**: All tool calls pass through SecurityEngine

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::Config;
use crate::memory::{MemoryStore, Message as MemoryMessage, ToolCall as MemoryToolCall};
use crate::providers::{
    ChatRequest, ChatResponse, Message, Provider, StreamChunk, ToolCallRequest, ToolDefinition,
    ToolFunction, StreamMetrics,
};
use crate::router::{route_task, RoutingDecision};
use crate::security::{SecurityDecision, SecurityEngine};
use crate::skills::{SkillRegistry, format_skills_prompt};
use crate::tools::{execute_tool, get_openai_tool_definitions, normalize_tool_args};

pub mod engine;
pub mod response;

pub use engine::HarnessEngine;
pub use response::{HarnessResponse, ModelResponse, ToolExecutionResult};
