pub mod codex_auth;
pub mod xai_auth;

use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::cache::{ResponseCache, compute_cache_key};
use crate::config::ProviderKind;

/// Global counter for total tokens spent across all providers (session-wide).
static TOTAL_TOKENS_SPENT: AtomicU64 = AtomicU64::new(0);
static TOTAL_ESTIMATED_COST_USD: AtomicU64 = AtomicU64::new(0);

/// Per-request cost tracking info.
#[derive(Debug, Clone)]
pub struct CostInfo {
    #[allow(dead_code)]
    pub prompt_tokens: u32,
    #[allow(dead_code)]
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// Estimated cost in USD (input + output).
    pub estimated_cost_usd: f64,
}

/// Increment global token/cost counters.
pub fn track_usage(cost: &CostInfo) {
    TOTAL_TOKENS_SPENT.fetch_add(cost.total_tokens as u64, Ordering::Relaxed);
    let cents = (cost.estimated_cost_usd * 1_000_000.0) as u64;
    TOTAL_ESTIMATED_COST_USD.fetch_add(cents, Ordering::Relaxed);
}

/// Get session-wide totals.
pub fn get_session_usage() -> (u64, f64) {
    let tokens = TOTAL_TOKENS_SPENT.load(Ordering::Relaxed);
    let cost_cents = TOTAL_ESTIMATED_COST_USD.load(Ordering::Relaxed);
    (tokens, cost_cents as f64 / 1_000_000.0)
}

/// Reset session-wide counters.
#[allow(dead_code)]
pub fn reset_session_usage() {
    TOTAL_TOKENS_SPENT.store(0, Ordering::Relaxed);
    TOTAL_ESTIMATED_COST_USD.store(0, Ordering::Relaxed);
}

/// Estimate cost in USD based on model name and token counts.
fn estimate_cost(model: &str, prompt_tokens: u32, completion_tokens: u32) -> f64 {
    let (input_per_1k, output_per_1k): (f64, f64) = match model {
        m if m.contains("gpt-4o") => (0.0025, 0.01),
        m if m.contains("gpt-4") => (0.03, 0.06),
        m if m.contains("gpt-3.5") || m.contains("gpt-35") => (0.0005, 0.0015),
        m if m.contains("claude-3-opus") => (0.015, 0.075),
        m if m.contains("claude-3-sonnet") => (0.003, 0.015),
        m if m.contains("claude-3-haiku") => (0.00025, 0.00125),
        m if m.contains("claude") => (0.003, 0.015),
        m if m.contains("gemini-1.5-pro") => (0.00125, 0.005),
        m if m.contains("gemini") => (0.00035, 0.0014),
        m if m.contains("deepseek") => (0.00014, 0.00028),
        m if m.contains("llama") || m.contains("qwen") => (0.0, 0.0),
        _ => (0.001, 0.003),
    };
    let prompt_cost = prompt_tokens as f64 / 1000.0 * input_per_1k;
    let completion_cost = completion_tokens as f64 / 1000.0 * output_per_1k;
    prompt_cost + completion_cost
}

/// A chunk from a streaming response, tagged as reasoning or content.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Reasoning/thinking content (shown in real-time before response).
    Reasoning(String),
    /// Actual response content.
    Content(String),
    /// Tool call from the model (OpenAI function calling format).
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// Streaming is complete with a finish reason.
    Finish(String),
}

/// Text from an OpenAI-style `content` value: a plain string, or an array of
/// content parts (`{"type": "text" | "output_text", "text": "..."}`).
fn openai_text(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(parts) => {
            let text: String = parts
                .iter()
                .filter_map(|p| match p {
                    serde_json::Value::String(s) => Some(s.as_str()),
                    _ => p.get("text").and_then(|t| t.as_str()),
                })
                .collect();
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}

/// Error message carried in an OpenAI-compatible payload that arrived with a
/// 2xx status, e.g. `{"error": {"message": ...}}` or xAI's
/// `{"code": ..., "error": "..."}`.
fn openai_stream_error(event: &serde_json::Value) -> Option<String> {
    let err = event.get("error").filter(|e| !e.is_null());
    let message = match err {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(e) => e
            .get("message")
            .and_then(|m| m.as_str())
            .map(|m| m.to_string())
            .unwrap_or_else(|| e.to_string()),
        None if event.get("type").and_then(|t| t.as_str()) == Some("error") => event
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown stream error")
            .to_string(),
        None => return None,
    };
    let code = event
        .get("code")
        .or_else(|| err.and_then(|e| e.get("code")))
        .and_then(|c| match c {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        });
    Some(match code {
        Some(code) if !message.contains(&code) => format!("{code}: {message}"),
        _ => message,
    })
}

const MAX_UNPARSED_BODY: usize = 8 * 1024;

fn push_unparsed(buf: &mut String, line: &str) {
    if buf.len() < MAX_UNPARSED_BODY {
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
    }
}

/// Chunks to send when a 2xx OpenAI-compatible response produced no content,
/// reasoning or tool call. `unparsed` is body text that was not an SSE
/// `data:` chunk. A plain JSON completion (the server ignored `stream`) is
/// replayed; an error body, or anything else, becomes a visible
/// `[API error: ...]` so the turn never ends as a silent empty reply.
fn recover_unstreamed_body(unparsed: &str) -> Vec<StreamChunk> {
    let body = unparsed.trim();
    if body.is_empty() {
        crate::debug_log("provider stream ended with no content, reasoning or tool calls");
        return Vec::new();
    }
    crate::debug_log(&format!(
        "provider stream had no SSE output; body: {}",
        crate::utils::truncate_str(body, 500)
    ));
    let mut out = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(err) = openai_stream_error(&json) {
            out.push(StreamChunk::Content(format!("[API error: {}]", err)));
            out.push(StreamChunk::Finish("error".to_string()));
            return out;
        }
        if let Some(message) = json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
        {
            if let Some(r) = message
                .get("reasoning_content")
                .and_then(|r| r.as_str())
                .filter(|r| !r.is_empty())
            {
                out.push(StreamChunk::Reasoning(r.to_string()));
            }
            if let Some(c) = openai_text(message.get("content")).filter(|c| !c.is_empty()) {
                out.push(StreamChunk::Content(c));
            }
            for tc in message
                .get("tool_calls")
                .and_then(|t| t.as_array())
                .into_iter()
                .flatten()
            {
                let f = tc.get("function");
                let field = |v: Option<&serde_json::Value>| {
                    v.and_then(|x| x.as_str()).unwrap_or_default().to_string()
                };
                out.push(StreamChunk::ToolCall {
                    id: field(tc.get("id")),
                    name: field(f.and_then(|f| f.get("name"))),
                    arguments: field(f.and_then(|f| f.get("arguments"))),
                });
            }
            if !out.is_empty() {
                return out;
            }
        }
    }
    out.push(StreamChunk::Content(format!(
        "[API error: unexpected response from provider: {}]",
        crate::utils::truncate_str(body, 300)
    )));
    out.push(StreamChunk::Finish("error".to_string()));
    out
}

/// If `value` is an unresolved `${VAR}` placeholder, return the variable name.
pub fn unresolved_env_placeholder(value: &str) -> Option<&str> {
    let v = value.trim();
    if v.len() > 3 && v.starts_with("${") && v.ends_with('}') {
        let name = &v[2..v.len() - 1];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Some(name);
        }
    }
    None
}

/// Returns true when an API error body indicates the prompt exceeded the
/// model's context window (OpenAI, Anthropic, llama.cpp, Ollama, vLLM, ...).
pub fn is_context_overflow_error(body: &str) -> bool {
    let b = body.to_ascii_lowercase();
    [
        "context_length_exceeded",
        "exceed_context_size",
        "exceeds the available context size",
        "maximum context length",
        "context window",
        "prompt is too long",
        "too many tokens",
        "exceeds the context",
        "context size exceeded",
        "input is too long",
    ]
    .iter()
    .any(|needle| b.contains(needle))
}

/// Format an HTTP error from a provider into a user-facing message, adding
/// an actionable hint for context-overflow errors.
pub fn format_api_error(status: reqwest::StatusCode, body: &str) -> String {
    if is_context_overflow_error(body) {
        format!(
            "Context window exceeded (API error {}): the prompt is larger than the model's \
             context size. OpenShield's system prompt alone is ~5k tokens. Raise the server's \
             context length (e.g. Ollama `PARAMETER num_ctx 16384`, llama.cpp `-c 16384`) or \
             use a model with a larger context. Server said: {}",
            status,
            body.trim()
        )
    } else {
        format!("API error {}: {}", status, body.trim())
    }
}

/// Accumulated tool call state for streaming fragments.
#[derive(Debug, Clone, Default)]
pub struct AccumulatedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// Optional image attachments as base64 data URLs.
    /// When present, the message is sent as multimodal content.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub images: Option<Vec<String>>,
    /// Required for role="tool" messages — matches the id from the assistant's tool_calls.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
    /// Required for role="assistant" messages that initiate tool calls.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<ToolCallRequest>>,
    /// Reasoning/thinking content (Kimi thinking, DeepSeek V4).
    /// Must be echoed back in history for models that require reasoning_content.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_content: Option<String>,
}

/// A tool call initiated by the assistant (OpenAI function calling format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub r#type: String,
    pub function: ToolCallFunction,
}

/// Function details within a tool call request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

impl Message {
    #[allow(dead_code)]
    /// Create a simple text message.
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }
    }

    /// Create a message with text and an image attachment.
    pub fn with_image(
        role: impl Into<String>,
        content: impl Into<String>,
        image_data_url: impl Into<String>,
    ) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            images: Some(vec![image_data_url.into()]),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }
    }

    /// Convert this message to the OpenAI multimodal format.
    /// Returns a JSON value representing the content array.
    pub fn to_openai_content(&self) -> serde_json::Value {
        if let Some(ref images) = self.images {
            let mut parts = vec![json!({"type": "text", "text": self.content})];
            for img in images {
                parts.push(json!({
                    "type": "image_url",
                    "image_url": {"url": img}
                }));
            }
            json!(parts)
        } else {
            json!(self.content)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Optional tools for function calling (OpenAI-compatible format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
}

/// OpenAI-compatible tool definition for function calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub r#type: String,
    pub function: ToolFunction,
}

/// Function definition within a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ChatRequest {
    pub fn new(model: String, messages: Vec<Message>, stream: bool) -> Self {
        Self {
            model,
            messages,
            stream,
            max_tokens: None,
            temperature: None,
            tools: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub message: Message,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct StreamMetrics {
    pub first_token_latency_ms: u64,
    pub total_latency_ms: u64,
    pub tokens_generated: u32,
    pub cached: bool,
}

#[derive(Debug, Clone)]
pub struct Provider {
    #[allow(dead_code)]
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub kind: ProviderKind,
    pub headers: HashMap<String, String>,
    client: reqwest::Client,
    cache: Option<ResponseCache>,
}

impl Provider {
    pub fn new(
        name: String,
        base_url: String,
        api_key: String,
        kind: ProviderKind,
        headers: HashMap<String, String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(std::time::Duration::from_secs(120))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            name,
            base_url,
            api_key,
            kind,
            headers,
            client,
            cache: ResponseCache::new().ok(),
        }
    }

    #[allow(dead_code)]
    pub fn new_without_cache(
        name: String,
        base_url: String,
        api_key: String,
        kind: ProviderKind,
        headers: HashMap<String, String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(std::time::Duration::from_secs(120))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            name,
            base_url,
            api_key,
            kind,
            headers,
            client,
            cache: None,
        }
    }

    /// Fail fast when the API key is still an unresolved `${VAR}` placeholder,
    /// instead of sending the literal placeholder to the provider.
    pub fn ensure_api_key_resolved(&self) -> Result<()> {
        if matches!(
            self.kind,
            ProviderKind::OpenAiCompatible | ProviderKind::Gemini | ProviderKind::Anthropic
        ) && let Some(var) = unresolved_env_placeholder(&self.api_key)
        {
            anyhow::bail!(
                "API key for provider '{}' is not set: environment variable {} is undefined. \
                 Export {} (or set `env_file`), or for local servers such as Ollama set \
                 `api_key` to any non-empty dummy value.",
                self.name,
                var,
                var
            );
        }
        Ok(())
    }

    /// Build the request builder with appropriate auth and headers for this provider kind.
    fn build_request_builder(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let mut builder = self.client.request(method, &url);

        match self.kind {
            ProviderKind::OpenAiCompatible | ProviderKind::Gemini => {
                builder = builder.header("Authorization", format!("Bearer {}", self.api_key));
            }
            ProviderKind::Anthropic => {
                builder = builder.header("x-api-key", &self.api_key);
                builder = builder.header("anthropic-version", "2023-06-01");
            }
            ProviderKind::OpenAiCodex | ProviderKind::XaiOauth => {
                // Auth headers require an async OAuth token refresh —
                // applied by the async call sites via apply_codex_auth()
                // or apply_xai_auth().
            }
        }

        // Add custom headers from config
        for (key, val) in &self.headers {
            builder = builder.header(key, val);
        }

        builder
    }

    /// Endpoint path for chat requests, per provider kind.
    fn chat_path(&self) -> &'static str {
        match self.kind {
            ProviderKind::OpenAiCodex => "/responses",
            _ => "/chat/completions",
        }
    }

    /// Apply Codex OAuth headers (Bearer token, account id, Responses API
    /// beta headers, session id) after ensuring the token is fresh.
    /// `force_refresh` re-fetches a new token after a 401.
    async fn apply_codex_auth(
        &self,
        builder: reqwest::RequestBuilder,
        force_refresh: bool,
    ) -> Result<reqwest::RequestBuilder> {
        let creds = codex_auth::ensure_fresh(force_refresh).await?;
        Ok(builder
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("chatgpt-account-id", &creds.account_id)
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "codex_cli_rs")
            .header("Accept", "text/event-stream")
            .header("session_id", codex_auth::session_id()))
    }

    /// Apply a fresh SuperGrok bearer. `force_refresh` re-fetches after a 401.
    async fn apply_xai_auth(
        &self,
        builder: reqwest::RequestBuilder,
        force_refresh: bool,
    ) -> Result<reqwest::RequestBuilder> {
        let token = xai_auth::ensure_fresh(force_refresh).await?;
        Ok(builder.header("Authorization", format!("Bearer {token}")))
    }

    /// Convert messages to the provider's native format.
    fn build_chat_body(&self, request: &ChatRequest) -> serde_json::Value {
        match self.kind {
            ProviderKind::OpenAiCompatible | ProviderKind::XaiOauth => {
                // Merge ALL system-role messages into a single leading system
                // message. Strict chat templates (stock Qwen3 / llama.cpp
                // minijinja, incl. the custom synthclaw templates) hard-fail
                // with "System message must be at the beginning" when any
                // second system message exists — and several OpenShield paths
                // inject memory/skills/compaction/truncation context as extra
                // system messages (harness double-prepend, Discord router,
                // gateway /v1/chat, follow-up truncation notices). Merging
                // preserves all content and is accepted by every
                // OpenAI-compatible backend.
                let system_count = request
                    .messages
                    .iter()
                    .filter(|m| m.role == "system")
                    .count();
                let normalized: Vec<Message> = if system_count > 1 {
                    let merged_system = request
                        .messages
                        .iter()
                        .filter(|m| m.role == "system")
                        .map(|m| m.content.trim())
                        .filter(|c| !c.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    let mut v: Vec<Message> = Vec::with_capacity(request.messages.len());
                    v.push(Message {
                        role: "system".to_string(),
                        content: merged_system,
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                    });
                    v.extend(
                        request
                            .messages
                            .iter()
                            .filter(|m| m.role != "system")
                            .cloned(),
                    );
                    v
                } else {
                    request.messages.clone()
                };
                let messages: Vec<_> = normalized
                    .iter()
                    .filter(|m| {
                        // Filter out empty assistant messages that have no tool_calls.
                        // Tool messages must always be sent (API requires content field).
                        // Assistant messages with tool_calls are kept (content becomes null).
                        if m.role == "assistant"
                            && m.content.trim().is_empty()
                            && m.tool_calls.is_none()
                        {
                            return false;
                        }
                        true
                    })
                    .map(|m| {
                        // For assistant messages with tool_calls, only force content to null
                        // if the content is actually empty. If the model output text before the
                        // tool call, preserve it so the API receives the full context.
                        let content = if m.role == "assistant"
                            && m.tool_calls.is_some()
                            && m.content.trim().is_empty()
                        {
                            serde_json::Value::Null
                        } else {
                            m.to_openai_content()
                        };
                        let mut msg = json!({
                            "role": m.role,
                            "content": content,
                        });
                        if let Some(ref tool_call_id) = m.tool_call_id {
                            msg["tool_call_id"] = json!(tool_call_id);
                        }
                        if let Some(ref tool_calls) = m.tool_calls {
                            msg["tool_calls"] = json!(tool_calls);
                        }
                        if let Some(ref reasoning) = m.reasoning_content {
                            msg["reasoning_content"] = json!(reasoning);
                        }
                        msg
                    })
                    .collect();

                let mut body = json!({
                    "model": request.model,
                    "messages": messages,
                    "stream": request.stream,
                });
                if let Some(max_tokens) = request.max_tokens {
                    body["max_tokens"] = json!(max_tokens);
                }
                if let Some(temp) = request.temperature {
                    body["temperature"] = json!(temp);
                }
                if let Some(ref tools) = request.tools {
                    body["tools"] = json!(tools);
                    body["tool_choice"] = json!("auto");
                }
                body
            }
            ProviderKind::Anthropic => {
                let system_msg = request
                    .messages
                    .iter()
                    .find(|m| m.role == "system")
                    .map(|m| m.content.clone());

                let messages: Vec<_> = request
                    .messages
                    .iter()
                    .filter(|m| m.role != "system")
                    .filter(|m| {
                        if (m.role == "assistant" || m.role == "tool")
                            && m.content.trim().is_empty()
                            && m.tool_calls.is_none()
                        {
                            return false;
                        }
                        true
                    })
                    .map(|m| {
                        // Anthropic uses the same multimodal format as OpenAI
                        json!({"role": m.role, "content": m.to_openai_content()})
                    })
                    .collect();

                let mut body = json!({
                    "model": request.model,
                    "messages": messages,
                    "stream": request.stream,
                    "max_tokens": request.max_tokens.unwrap_or(4096),
                });

                if let Some(system) = system_msg {
                    body["system"] = json!(system);
                }

                body
            }
            ProviderKind::Gemini => {
                let contents: Vec<_> = request.messages.iter()
                    .filter(|m| {
                        if (m.role == "assistant" || m.role == "tool") && m.content.trim().is_empty() && m.tool_calls.is_none() {
                            return false;
                        }
                        true
                    })
                    .map(|m| {
                        let role = if m.role == "assistant" { "model" } else { &m.role };
                        let mut parts = vec![json!({"text": m.content})];
                        if let Some(ref images) = m.images {
                            for img in images {
                                parts.push(json!({
                                    "inline_data": {
                                        "mime_type": "image/png",
                                        "data": img.strip_prefix("data:image/png;base64,").unwrap_or(img)
                                    }
                                }));
                            }
                        }
                        json!({
                            "role": role,
                            "parts": parts,
                        })
                    })
                    .collect();

                json!({
                    "contents": contents,
                })
            }
            ProviderKind::OpenAiCodex => {
                // OpenAI Responses API (chatgpt.com/backend-api/codex/responses).
                // System-role content goes into the top-level "instructions"
                // field (merged like the OpenAiCompatible arm — the harness
                // can emit multiple system messages); conversation turns go
                // into "input" as Responses API items.
                let instructions = request
                    .messages
                    .iter()
                    .filter(|m| m.role == "system")
                    .map(|m| m.content.trim())
                    .filter(|c| !c.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");

                let mut input: Vec<serde_json::Value> = Vec::new();
                for m in request.messages.iter().filter(|m| m.role != "system") {
                    match m.role.as_str() {
                        "user" => {
                            let mut parts = vec![json!({"type": "input_text", "text": m.content})];
                            if let Some(ref images) = m.images {
                                for img in images {
                                    parts.push(json!({
                                        "type": "input_image",
                                        "image_url": img
                                    }));
                                }
                            }
                            input.push(json!({
                                "type": "message",
                                "role": "user",
                                "content": parts,
                            }));
                        }
                        "assistant" => {
                            if !m.content.trim().is_empty() {
                                input.push(json!({
                                    "type": "message",
                                    "role": "assistant",
                                    "content": [{"type": "output_text", "text": m.content}],
                                }));
                            }
                            if let Some(ref tool_calls) = m.tool_calls {
                                for tc in tool_calls {
                                    input.push(json!({
                                        "type": "function_call",
                                        "call_id": tc.id,
                                        "name": tc.function.name,
                                        "arguments": tc.function.arguments,
                                    }));
                                }
                            }
                        }
                        "tool" => {
                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": m.tool_call_id.clone().unwrap_or_default(),
                                "output": m.content,
                            }));
                        }
                        _ => {}
                    }
                }

                let mut body = json!({
                    "model": request.model,
                    "instructions": instructions,
                    "input": input,
                    "tool_choice": "auto",
                    "parallel_tool_calls": false,
                    "reasoning": {"effort": "medium", "summary": "auto"},
                    "store": false,
                    "stream": request.stream,
                    "include": [],
                });
                if let Some(ref tools) = request.tools {
                    // Convert OpenAI chat-completions tool defs to Responses
                    // API function tools.
                    let responses_tools: Vec<serde_json::Value> = tools
                        .iter()
                        .map(|t| {
                            json!({
                                "type": "function",
                                "name": t.function.name,
                                "description": t.function.description,
                                "strict": false,
                                "parameters": t.function.parameters,
                            })
                        })
                        .collect();
                    body["tools"] = json!(responses_tools);
                }
                body
            }
        }
    }

    /// Parse a response from the provider's native format into our ChatResponse.
    /// Also extracts Kimi thinking content from `<think>...</think>` tags.
    fn parse_chat_response(&self, body: &str) -> Result<ChatResponse> {
        match self.kind {
            ProviderKind::OpenAiCompatible | ProviderKind::XaiOauth => {
                // First try standard OpenAI format
                if let Ok(response) = serde_json::from_str::<ChatResponse>(body) {
                    return Ok(response);
                }
                // Fallback: try to parse Kimi-style response with reasoning_content
                let raw: serde_json::Value = serde_json::from_str(body).with_context(|| {
                    format!("Failed to parse OpenAI-compatible response: {}", body)
                })?;

                let mut content = raw["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                // Extract tool_calls if present
                let tool_calls = raw["choices"][0]["message"]["tool_calls"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|tc| {
                                let id = tc["id"].as_str()?.to_string();
                                let tool_type =
                                    tc["type"].as_str().unwrap_or("function").to_string();
                                let function_name = tc["function"]["name"].as_str()?.to_string();
                                let function_args =
                                    tc["function"]["arguments"].as_str()?.to_string();
                                Some(ToolCallRequest {
                                    id,
                                    r#type: tool_type,
                                    function: ToolCallFunction {
                                        name: function_name,
                                        arguments: function_args,
                                    },
                                })
                            })
                            .collect::<Vec<_>>()
                    });

                if let Some(reasoning) = raw["choices"][0]["message"]["reasoning_content"].as_str()
                    && !reasoning.is_empty()
                {
                    content = format!("<think>\n{}\n</think>\n\n{}", reasoning, content);
                }

                let usage = raw.get("usage").map(|u| Usage {
                    prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                    completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                    total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
                });

                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: Message {
                            role: "assistant".to_string(),
                            content,
                            images: None,
                            tool_call_id: None,
                            tool_calls,
                            reasoning_content: None,
                        },
                        finish_reason: raw["choices"][0]["finish_reason"]
                            .as_str()
                            .map(|s| s.to_string()),
                    }],
                    usage,
                })
            }
            ProviderKind::Anthropic => {
                let raw: serde_json::Value = serde_json::from_str(body)
                    .with_context(|| format!("Failed to parse Anthropic response: {}", body))?;

                let content = raw["content"][0]["text"]
                    .as_str()
                    .or_else(|| raw["content"].as_str())
                    .unwrap_or("")
                    .to_string();

                let usage = raw.get("usage").map(|u| Usage {
                    prompt_tokens: u["input_tokens"].as_u64().unwrap_or(0) as u32,
                    completion_tokens: u["output_tokens"].as_u64().unwrap_or(0) as u32,
                    total_tokens: u["input_tokens"].as_u64().unwrap_or(0) as u32
                        + u["output_tokens"].as_u64().unwrap_or(0) as u32,
                });

                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: Message {
                            role: "assistant".to_string(),
                            content,
                            images: None,
                            tool_call_id: None,
                            tool_calls: None,
                            reasoning_content: None,
                        },
                        finish_reason: raw["stop_reason"].as_str().map(|s| s.to_string()),
                    }],
                    usage,
                })
            }
            ProviderKind::Gemini => {
                let raw: serde_json::Value = serde_json::from_str(body)
                    .with_context(|| format!("Failed to parse Gemini response: {}", body))?;

                let content = raw["candidates"][0]["content"]["parts"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let usage = raw.get("usageMetadata").map(|u| Usage {
                    prompt_tokens: u["promptTokenCount"].as_u64().unwrap_or(0) as u32,
                    completion_tokens: u["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
                    total_tokens: u["totalTokenCount"].as_u64().unwrap_or(0) as u32,
                });

                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: Message {
                            role: "assistant".to_string(),
                            content,
                            images: None,
                            tool_call_id: None,
                            tool_calls: None,
                            reasoning_content: None,
                        },
                        finish_reason: Some("stop".to_string()),
                    }],
                    usage,
                })
            }
            ProviderKind::OpenAiCodex => {
                // Responses API non-streaming response: output array holds
                // message items (output_text) and function_call items.
                let raw: serde_json::Value = serde_json::from_str(body)
                    .with_context(|| format!("Failed to parse Codex response: {}", body))?;

                let mut content = String::new();
                let mut tool_calls: Vec<ToolCallRequest> = Vec::new();
                if let Some(items) = raw.get("output").and_then(|o| o.as_array()) {
                    for item in items {
                        match item.get("type").and_then(|t| t.as_str()) {
                            Some("message") => {
                                if let Some(parts) = item.get("content").and_then(|c| c.as_array())
                                {
                                    for part in parts {
                                        if part.get("type").and_then(|t| t.as_str())
                                            == Some("output_text")
                                            && let Some(text) =
                                                part.get("text").and_then(|t| t.as_str())
                                        {
                                            content.push_str(text);
                                        }
                                    }
                                }
                            }
                            Some("function_call") => {
                                let id = item
                                    .get("call_id")
                                    .and_then(|c| c.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let name = item
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let arguments = item
                                    .get("arguments")
                                    .and_then(|a| a.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                tool_calls.push(ToolCallRequest {
                                    id,
                                    r#type: "function".to_string(),
                                    function: ToolCallFunction { name, arguments },
                                });
                            }
                            _ => {}
                        }
                    }
                }

                let usage = raw.get("usage").map(|u| {
                    let prompt = u["input_tokens"].as_u64().unwrap_or(0) as u32;
                    let completion = u["output_tokens"].as_u64().unwrap_or(0) as u32;
                    Usage {
                        prompt_tokens: prompt,
                        completion_tokens: completion,
                        total_tokens: u["total_tokens"]
                            .as_u64()
                            .map(|t| t as u32)
                            .unwrap_or(prompt + completion),
                    }
                });

                let finish_reason = if tool_calls.is_empty() {
                    Some("stop".to_string())
                } else {
                    Some("tool_calls".to_string())
                };

                Ok(ChatResponse {
                    choices: vec![Choice {
                        message: Message {
                            role: "assistant".to_string(),
                            content,
                            images: None,
                            tool_call_id: None,
                            tool_calls: if tool_calls.is_empty() {
                                None
                            } else {
                                Some(tool_calls)
                            },
                            reasoning_content: None,
                        },
                        finish_reason,
                    }],
                    usage,
                })
            }
        }
    }

    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let messages_json = serde_json::to_string(&request.messages)
            .with_context(|| "Failed to serialize messages for cache key")?;
        let cache_key = compute_cache_key(&request.model, &messages_json);

        if let Some(ref cache) = self.cache
            && let Some(cached) = cache.get(&cache_key)
        {
            let chat_response = self.parse_chat_response(&cached.response)?;
            return Ok(chat_response);
        }

        let response = self.send_with_retry(&request, 3).await?;
        let body_text = response
            .text()
            .await
            .with_context(|| "Failed to read response body")?;

        let chat_response = self.parse_chat_response(&body_text)?;

        // Track cost
        if let Some(ref usage) = chat_response.usage {
            let cost = CostInfo {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                estimated_cost_usd: estimate_cost(
                    &request.model,
                    usage.prompt_tokens,
                    usage.completion_tokens,
                ),
            };
            track_usage(&cost);
        }

        if let Some(ref cache) = self.cache {
            let ttl_secs = if request.stream { 3600 } else { 86400 };
            let _ = cache.set(&cache_key, &body_text, ttl_secs);
        }

        Ok(chat_response)
    }

    /// Retry a request with exponential backoff on rate limits (429) and transient errors (5xx).
    async fn send_with_retry(
        &self,
        request: &ChatRequest,
        max_retries: u32,
    ) -> Result<reqwest::Response> {
        self.ensure_api_key_resolved()?;
        let body = self.build_chat_body(request);
        let mut last_error = String::new();
        // For Codex: after a 401, force one token refresh and retry.
        let mut oauth_force_refresh = false;

        for attempt in 0..=max_retries {
            if attempt > 0 {
                let delay_ms = 500u64 * (2u64.pow(attempt - 1)).min(30000);
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }

            let mut builder = self
                .build_request_builder(reqwest::Method::POST, self.chat_path())
                .header("Content-Type", "application/json")
                .json(&body);
            if self.kind == ProviderKind::OpenAiCodex {
                builder = self.apply_codex_auth(builder, oauth_force_refresh).await?;
            } else if self.kind == ProviderKind::XaiOauth {
                builder = self.apply_xai_auth(builder, oauth_force_refresh).await?;
            }

            let response = match builder.send().await {
                Ok(r) => r,
                Err(e) => {
                    last_error = e.to_string();
                    if e.is_timeout() || e.is_connect() {
                        continue;
                    }
                    return Err(e)
                        .with_context(|| format!("Request failed after {} retries", attempt));
                }
            };

            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }

            let body_text = response.text().await.unwrap_or_default();
            last_error = format_api_error(status, &body_text);

            // Codex: on 401, force a token refresh once and retry.
            if status.as_u16() == 401
                && matches!(
                    self.kind,
                    ProviderKind::OpenAiCodex | ProviderKind::XaiOauth
                )
                && !oauth_force_refresh
            {
                oauth_force_refresh = true;
                continue;
            }

            // Retry on rate limit or server errors
            if status.as_u16() == 429 || status.is_server_error() {
                continue;
            }

            // Client errors (4xx except 429) are not retryable
            anyhow::bail!("{}", last_error);
        }

        anyhow::bail!(
            "Request failed after {} retries: {}",
            max_retries,
            last_error
        )
    }

    pub async fn chat_stream(&self, request: ChatRequest) -> Result<(Vec<String>, StreamMetrics)> {
        let start_time = Instant::now();
        let messages_json = serde_json::to_string(&request.messages)
            .with_context(|| "Failed to serialize messages for cache key")?;
        let cache_key = compute_cache_key(&request.model, &messages_json);
        let has_tools = request.tools.is_some();

        if !has_tools
            && let Some(ref cache) = self.cache
            && let Some(cached) = cache.get(&cache_key)
        {
            let chunks: Vec<String> = serde_json::from_str(&cached.response)
                .with_context(|| "Failed to parse cached stream response")?;
            let token_count = chunks.len() as u32;
            return Ok((
                chunks,
                StreamMetrics {
                    first_token_latency_ms: 0,
                    total_latency_ms: 0,
                    tokens_generated: token_count,
                    cached: true,
                },
            ));
        }

        let response = self.send_with_retry(&request, 3).await?;
        let mut stream = response.bytes_stream();
        let mut chunks = Vec::new();
        let mut first_token_time: Option<Instant> = None;
        let mut buffer = String::new();
        // FIXED: Use a fixed deadline so empty keepalive chunks don't reset the timer
        let stream_deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        // Prevent unbounded chunk accumulation for very long responses
        const MAX_CHUNKS: usize = 10_000;
        let mut merged_buffer = String::new();

        loop {
            let remaining = stream_deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                chunks.push("[⏱ Stream timed out after 120s of inactivity]".to_string());
                break;
            }
            let chunk_result = match tokio::time::timeout(remaining, stream.next()).await {
                Ok(Some(result)) => result,
                Ok(None) => break, // stream ended normally
                Err(_) => {
                    chunks.push("[⏱ Stream timed out after 120s of inactivity]".to_string());
                    break;
                }
            };

            // CRITICAL FIX: Don't kill the entire stream on a single chunk decode error.
            // Log it and continue — the next chunk may be fine. This prevents the harness
            // from dying on transient network hiccups or malformed SSE boundaries.
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("SSE chunk decode error (non-realtime): {}", e);
                    continue;
                }
            };
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete lines from buffer
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if let Some(data) = line.strip_prefix("data:") {
                    let data = data.trim_start();
                    if data == "[DONE]" {
                        continue;
                    }
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        let delta_content = match self.kind {
                            ProviderKind::OpenAiCompatible | ProviderKind::XaiOauth => {
                                // Stream both content and reasoning_content (Kimi thinking)
                                let content = event
                                    .get("choices")
                                    .and_then(|c| c.get(0))
                                    .and_then(|c| c.get("delta"))
                                    .and_then(|d| d.get("content"))
                                    .and_then(|c| c.as_str());
                                let reasoning = event
                                    .get("choices")
                                    .and_then(|c| c.get(0))
                                    .and_then(|c| c.get("delta"))
                                    .and_then(|d| d.get("reasoning_content"))
                                    .and_then(|c| c.as_str());
                                // If we have reasoning content, emit it as a special marker
                                if let Some(r) = reasoning
                                    && !r.is_empty()
                                {
                                    // Emit reasoning content wrapped in think tags so the TUI can display it
                                    chunks.push(format!("<think>{}</think>", r));
                                }
                                content
                            }
                            ProviderKind::Anthropic => event
                                .get("delta")
                                .and_then(|d| d.get("text"))
                                .and_then(|t| t.as_str())
                                .or_else(|| {
                                    event
                                        .get("content_block")
                                        .and_then(|c| c.get("text"))
                                        .and_then(|t| t.as_str())
                                }),
                            ProviderKind::Gemini => event
                                .get("candidates")
                                .and_then(|c| c.get(0))
                                .and_then(|c| c.get("content"))
                                .and_then(|c| c.get("parts"))
                                .and_then(|p| p.get(0))
                                .and_then(|p| p.get("text"))
                                .and_then(|t| t.as_str()),
                            ProviderKind::OpenAiCodex => {
                                // Responses API SSE: text arrives in
                                // response.output_text.delta events.
                                if event.get("type").and_then(|t| t.as_str())
                                    == Some("response.output_text.delta")
                                {
                                    event.get("delta").and_then(|d| d.as_str())
                                } else {
                                    None
                                }
                            }
                        };

                        if let Some(delta) = delta_content {
                            if first_token_time.is_none() {
                                first_token_time = Some(Instant::now());
                            }
                            // Merge chunks if we're approaching the limit to save RAM
                            if chunks.len() >= MAX_CHUNKS {
                                merged_buffer.push_str(&chunks.join(""));
                                chunks.clear();
                                chunks.push(merged_buffer.clone());
                                merged_buffer.clear();
                            }
                            chunks.push(delta.to_string());
                        }
                    }
                }
            }
        }

        let total_latency = start_time.elapsed();
        let first_token_latency = first_token_time
            .map(|t| t.duration_since(start_time))
            .unwrap_or_default();
        let token_count = chunks.len() as u32;

        if let Some(ref cache) = self.cache {
            let ttl_secs = 3600;
            if let Ok(body) = serde_json::to_string(&chunks) {
                let _ = cache.set(&cache_key, &body, ttl_secs);
            }
        }

        Ok((
            chunks,
            StreamMetrics {
                first_token_latency_ms: first_token_latency.as_millis() as u64,
                total_latency_ms: total_latency.as_millis() as u64,
                tokens_generated: token_count,
                cached: false,
            },
        ))
    }

    /// Real-time streaming: returns a receiver that yields `StreamChunk` values as they arrive.
    /// The caller (TUI task) can `recv().await` on the receiver to get chunks in real-time.
    pub async fn chat_stream_realtime(
        &self,
        request: ChatRequest,
    ) -> Result<(
        tokio::sync::mpsc::UnboundedReceiver<StreamChunk>,
        StreamMetrics,
    )> {
        let _start_time = Instant::now();
        let messages_json = serde_json::to_string(&request.messages)
            .with_context(|| "Failed to serialize messages for cache key")?;
        let cache_key = compute_cache_key(&request.model, &messages_json);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<StreamChunk>();

        // Check cache first — if cached, replay all chunks then return metrics
        // Skip caching for requests with tools to avoid replaying tool-call streams as plain content
        let has_tools = request.tools.is_some();
        if !has_tools
            && let Some(ref cache) = self.cache
            && let Some(cached) = cache.get(&cache_key)
            && let Ok(chunks) = serde_json::from_str::<Vec<String>>(&cached.response)
        {
            for chunk in chunks {
                let _ = tx.send(StreamChunk::Content(chunk));
            }
            return Ok((
                rx,
                StreamMetrics {
                    first_token_latency_ms: 0,
                    total_latency_ms: 0,
                    tokens_generated: 0,
                    cached: true,
                },
            ));
        }

        let body = self.build_chat_body(&request);
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let kind = self.kind.clone();
        let headers = self.headers.clone();

        self.ensure_api_key_resolved()?;
        // Send the request and check the HTTP status before spawning the stream
        // reader, so auth/HTTP failures surface as errors instead of being
        // injected into the stream as assistant content.
        let path = match kind {
            ProviderKind::OpenAiCodex => "/responses",
            _ => "/chat/completions",
        };
        let mut request_builder = client
            .request(
                reqwest::Method::POST,
                format!("{}{}", base_url.trim_end_matches('/'), path),
            )
            .header("Content-Type", "application/json")
            .json(&body);

        match kind {
            ProviderKind::OpenAiCompatible | ProviderKind::Gemini => {
                request_builder =
                    request_builder.header("Authorization", format!("Bearer {}", api_key));
            }
            ProviderKind::Anthropic => {
                request_builder = request_builder.header("x-api-key", &api_key);
                request_builder = request_builder.header("anthropic-version", "2023-06-01");
            }
            ProviderKind::OpenAiCodex => match codex_auth::ensure_fresh(false).await {
                Ok(creds) => {
                    request_builder = request_builder
                        .header("Authorization", format!("Bearer {}", creds.access_token))
                        .header("chatgpt-account-id", &creds.account_id)
                        .header("OpenAI-Beta", "responses=experimental")
                        .header("originator", "codex_cli_rs")
                        .header("Accept", "text/event-stream")
                        .header("session_id", codex_auth::session_id());
                }
                Err(e) => anyhow::bail!("codex auth error: {}", e),
            },
            ProviderKind::XaiOauth => match xai_auth::ensure_fresh(false).await {
                Ok(token) => {
                    request_builder =
                        request_builder.header("Authorization", format!("Bearer {token}"));
                }
                Err(e) => anyhow::bail!("xai auth error: {}", e),
            },
        }
        for (key, val) in &headers {
            request_builder = request_builder.header(key, val);
        }

        let response = request_builder
            .send()
            .await
            .with_context(|| format!("Request to provider '{}' failed", self.name))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            anyhow::bail!("{}", format_api_error(status, &body_text));
        }

        // Spawn the streaming work in a background task so the caller can start receiving immediately
        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            let mut first_token_time: Option<Instant> = None;
            // Raw bytes, split on '\n' before decoding: a multi-byte UTF-8
            // character split across two network chunks must not be mangled
            // into U+FFFD by decoding each chunk on its own.
            let mut buffer: Vec<u8> = Vec::new();
            let mut stream_done = false;
            // Did this response produce anything the caller can use? A 2xx
            // body with no content, reasoning or tool call is reported
            // instead of silently becoming an "empty response".
            let mut emitted_output = false;
            let mut content_seen = false;
            let mut stream_errored = false;
            // Body text that was not an SSE `data:` chunk we understood
            // (a plain JSON error or a non-streamed completion).
            let mut unparsed = String::new();
            // Accumulate tool call fragments across SSE chunks, keyed by index
            let mut pending_tool_calls: HashMap<u32, AccumulatedToolCall> = HashMap::new();

            // FIXED: Use a fixed deadline so empty keepalive chunks don't reset the timer
            let stream_deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            loop {
                let remaining =
                    stream_deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    let _ = tx.send(StreamChunk::Content(
                        "[⏱ Stream timed out after 120s of inactivity]".to_string(),
                    ));
                    break;
                }
                match tokio::time::timeout(remaining, stream.next()).await {
                    Ok(Some(Ok(c))) => buffer.extend_from_slice(&c),
                    Ok(Some(Err(e))) => {
                        // CRITICAL FIX: Log chunk error and continue instead of killing the stream.
                        // Transient decode errors (malformed SSE boundaries, network hiccups)
                        // should not abort the entire response.
                        tracing::warn!("SSE chunk decode error (realtime): {}", e);
                        continue;
                    }
                    Ok(None) => {
                        // Stream ended normally. Terminate a final line that
                        // arrived without a trailing newline so it is still
                        // processed below instead of being dropped.
                        stream_done = true;
                        buffer.push(b'\n');
                    }
                    Err(_) => {
                        let _ = tx.send(StreamChunk::Content(
                            "[⏱ Stream timed out after 120s of inactivity]".to_string(),
                        ));
                        break;
                    }
                };

                // Process complete lines from buffer
                while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();

                    if line.is_empty()
                        || line.starts_with(':')
                        || line.starts_with("event:")
                        || line.starts_with("id:")
                        || line.starts_with("retry:")
                    {
                        continue;
                    }
                    let Some(data) = line.strip_prefix("data:") else {
                        // Not SSE at all: keep it so an error body or a
                        // non-streamed completion can be reported at the end.
                        push_unparsed(&mut unparsed, &line);
                        continue;
                    };
                    {
                        let data = data.trim_start();
                        if data == "[DONE]" {
                            continue;
                        }
                        let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
                            push_unparsed(&mut unparsed, data);
                            continue;
                        };
                        {
                            match kind {
                                ProviderKind::OpenAiCompatible | ProviderKind::XaiOauth => {
                                    // An error delivered inside a 2xx stream
                                    // (`data: {"error": ...}`) used to be
                                    // skipped, so the turn looked like the
                                    // model had returned nothing.
                                    if let Some(err) = openai_stream_error(&event) {
                                        crate::debug_log(&format!(
                                            "provider stream error event: {}",
                                            crate::utils::truncate_str(data, 500)
                                        ));
                                        let _ = tx.send(StreamChunk::Content(format!(
                                            "[API error: {}]",
                                            err
                                        )));
                                        let _ = tx.send(StreamChunk::Finish("error".to_string()));
                                        stream_errored = true;
                                        continue;
                                    }
                                    let choice = event.get("choices").and_then(|c| c.get(0));
                                    let delta = choice.and_then(|c| c.get("delta"));
                                    // Text may arrive as a string or as an
                                    // array of content parts, and some
                                    // servers send whole `message` chunks
                                    // instead of deltas. That fallback only
                                    // applies while no delta text has arrived,
                                    // so a final chunk repeating the full
                                    // message cannot duplicate the reply.
                                    let content = openai_text(delta.and_then(|d| d.get("content")))
                                        .or_else(|| {
                                            if content_seen {
                                                return None;
                                            }
                                            openai_text(
                                                choice
                                                    .and_then(|c| c.get("message"))
                                                    .and_then(|m| m.get("content")),
                                            )
                                            .or_else(
                                                || openai_text(choice.and_then(|c| c.get("text"))),
                                            )
                                        });
                                    let content = content.as_deref();
                                    let reasoning = delta
                                        .and_then(|d| {
                                            d.get("reasoning_content")
                                                .or_else(|| d.get("reasoning"))
                                        })
                                        .and_then(|c| c.as_str());
                                    let tool_calls = delta
                                        .and_then(|d| d.get("tool_calls"))
                                        .and_then(|t| t.as_array());
                                    let finish_reason = event
                                        .get("choices")
                                        .and_then(|c| c.get(0))
                                        .and_then(|c| c.get("finish_reason"))
                                        .and_then(|f| f.as_str());

                                    // Accumulate tool call fragments from streaming deltas.
                                    // OpenAI sends tool_calls across multiple SSE events with
                                    // an `index` field; each chunk may contain partial id, name,
                                    // or arguments that must be concatenated.
                                    if let Some(tcs) = tool_calls {
                                        for tc in tcs {
                                            if let Some(idx) =
                                                tc.get("index").and_then(|i| i.as_u64())
                                            {
                                                let idx_u32 = idx as u32;
                                                let entry = pending_tool_calls
                                                    .entry(idx_u32)
                                                    .or_insert_with(|| AccumulatedToolCall {
                                                        id: String::new(),
                                                        name: String::new(),
                                                        arguments: String::new(),
                                                    });
                                                if let Some(id) =
                                                    tc.get("id").and_then(|i| i.as_str())
                                                {
                                                    entry.id.push_str(id);
                                                }
                                                if let Some(name) = tc
                                                    .get("function")
                                                    .and_then(|f| f.get("name"))
                                                    .and_then(|n| n.as_str())
                                                {
                                                    entry.name.push_str(name);
                                                }
                                                if let Some(args) = tc
                                                    .get("function")
                                                    .and_then(|f| f.get("arguments"))
                                                    .and_then(|a| a.as_str())
                                                {
                                                    entry.arguments.push_str(args);
                                                }
                                            } else if let Some(id) =
                                                tc.get("id").and_then(|i| i.as_str())
                                            {
                                                // Fallback for providers that omit index
                                                let entry = pending_tool_calls
                                                    .entry(0)
                                                    .or_insert_with(|| AccumulatedToolCall {
                                                        id: String::new(),
                                                        name: String::new(),
                                                        arguments: String::new(),
                                                    });
                                                entry.id.push_str(id);
                                                if let Some(name) = tc
                                                    .get("function")
                                                    .and_then(|f| f.get("name"))
                                                    .and_then(|n| n.as_str())
                                                {
                                                    entry.name.push_str(name);
                                                }
                                                if let Some(args) = tc
                                                    .get("function")
                                                    .and_then(|f| f.get("arguments"))
                                                    .and_then(|a| a.as_str())
                                                {
                                                    entry.arguments.push_str(args);
                                                }
                                            }
                                        }
                                    }

                                    // Emit content and reasoning FIRST, then Finish.
                                    // Consumers may break on Finish; if Finish is sent before
                                    // the final content/reasoning chunks, those chunks are lost.
                                    if let Some(r) = reasoning
                                        && !r.is_empty()
                                    {
                                        emitted_output = true;
                                        let _ = tx.send(StreamChunk::Reasoning(r.to_string()));
                                    }
                                    if let Some(c) = content
                                        && !c.is_empty()
                                    {
                                        if first_token_time.is_none() {
                                            first_token_time = Some(Instant::now());
                                        }
                                        emitted_output = true;
                                        content_seen = true;
                                        let _ = tx.send(StreamChunk::Content(c.to_string()));
                                    }

                                    // Emit finish reason LAST so all delta content is already sent
                                    if let Some(fr) = finish_reason
                                        && !fr.is_empty()
                                    {
                                        if fr == "tool_calls" {
                                            // Flush all accumulated tool calls
                                            for (_idx, tc) in pending_tool_calls.drain() {
                                                emitted_output = true;
                                                let _ = tx.send(StreamChunk::ToolCall {
                                                    id: tc.id,
                                                    name: tc.name,
                                                    arguments: tc.arguments,
                                                });
                                            }
                                            let _ = tx.send(StreamChunk::Finish(fr.to_string()));
                                        } else if fr == "stop"
                                            || fr == "end_turn"
                                            || fr == "length"
                                            || fr == "content_filter"
                                        {
                                            // Flush any remaining tool calls before sending stop
                                            for (_idx, tc) in pending_tool_calls.drain() {
                                                emitted_output = true;
                                                let _ = tx.send(StreamChunk::ToolCall {
                                                    id: tc.id,
                                                    name: tc.name,
                                                    arguments: tc.arguments,
                                                });
                                            }
                                            let _ = tx.send(StreamChunk::Finish(fr.to_string()));
                                        }
                                    }
                                }
                                ProviderKind::Anthropic => {
                                    let text = event
                                        .get("delta")
                                        .and_then(|d| d.get("text"))
                                        .and_then(|t| t.as_str())
                                        .or_else(|| {
                                            event
                                                .get("content_block")
                                                .and_then(|c| c.get("text"))
                                                .and_then(|t| t.as_str())
                                        });
                                    if let Some(t) = text
                                        && !t.is_empty()
                                    {
                                        if first_token_time.is_none() {
                                            first_token_time = Some(Instant::now());
                                        }
                                        let _ = tx.send(StreamChunk::Content(t.to_string()));
                                    }
                                }
                                ProviderKind::Gemini => {
                                    let text = event
                                        .get("candidates")
                                        .and_then(|c| c.get(0))
                                        .and_then(|c| c.get("content"))
                                        .and_then(|c| c.get("parts"))
                                        .and_then(|p| p.get(0))
                                        .and_then(|p| p.get("text"))
                                        .and_then(|t| t.as_str());
                                    if let Some(t) = text
                                        && !t.is_empty()
                                    {
                                        if first_token_time.is_none() {
                                            first_token_time = Some(Instant::now());
                                        }
                                        let _ = tx.send(StreamChunk::Content(t.to_string()));
                                    }
                                }
                                ProviderKind::OpenAiCodex => {
                                    // Responses API SSE: event kind is in the
                                    // JSON "type" field of each data payload.
                                    let event_type =
                                        event.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    match event_type {
                                        "response.output_text.delta" => {
                                            if let Some(d) =
                                                event.get("delta").and_then(|d| d.as_str())
                                                && !d.is_empty()
                                            {
                                                if first_token_time.is_none() {
                                                    first_token_time = Some(Instant::now());
                                                }
                                                let _ =
                                                    tx.send(StreamChunk::Content(d.to_string()));
                                            }
                                        }
                                        "response.reasoning_summary_text.delta" => {
                                            if let Some(d) =
                                                event.get("delta").and_then(|d| d.as_str())
                                                && !d.is_empty()
                                            {
                                                let _ =
                                                    tx.send(StreamChunk::Reasoning(d.to_string()));
                                            }
                                        }
                                        "response.output_item.done" => {
                                            // Completed items may be function calls.
                                            let item = event.get("item");
                                            if item
                                                .and_then(|i| i.get("type"))
                                                .and_then(|t| t.as_str())
                                                == Some("function_call")
                                            {
                                                let id = item
                                                    .and_then(|i| i.get("call_id"))
                                                    .and_then(|c| c.as_str())
                                                    .unwrap_or_default()
                                                    .to_string();
                                                let name = item
                                                    .and_then(|i| i.get("name"))
                                                    .and_then(|n| n.as_str())
                                                    .unwrap_or_default()
                                                    .to_string();
                                                let arguments = item
                                                    .and_then(|i| i.get("arguments"))
                                                    .and_then(|a| a.as_str())
                                                    .unwrap_or_default()
                                                    .to_string();
                                                let _ = tx.send(StreamChunk::ToolCall {
                                                    id,
                                                    name,
                                                    arguments,
                                                });
                                            }
                                        }
                                        "response.completed" => {
                                            let _ =
                                                tx.send(StreamChunk::Finish("stop".to_string()));
                                        }
                                        "response.failed" | "error" => {
                                            let msg = event
                                                .get("response")
                                                .and_then(|r| r.get("error"))
                                                .and_then(|e| e.get("message"))
                                                .and_then(|m| m.as_str())
                                                .or_else(|| {
                                                    event
                                                        .get("error")
                                                        .and_then(|e| e.get("message"))
                                                        .and_then(|m| m.as_str())
                                                })
                                                .or_else(|| {
                                                    event.get("message").and_then(|m| m.as_str())
                                                })
                                                .unwrap_or("unknown codex stream error");
                                            let _ = tx.send(StreamChunk::Content(format!(
                                                "[API error: {}]",
                                                msg
                                            )));
                                            let _ =
                                                tx.send(StreamChunk::Finish("error".to_string()));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
                if stream_done {
                    break;
                }
            }
            // Stream ended normally — send Finish so the TUI knows we're done
            // Flush any remaining tool calls that weren't sent due to missing finish_reason
            for (_idx, tc) in pending_tool_calls.drain() {
                emitted_output = true;
                let _ = tx.send(StreamChunk::ToolCall {
                    id: tc.id,
                    name: tc.name,
                    arguments: tc.arguments,
                });
            }
            if matches!(
                kind,
                ProviderKind::OpenAiCompatible | ProviderKind::XaiOauth
            ) && !emitted_output
                && !stream_errored
            {
                // Nothing usable came back in a 2xx response. Recover what
                // we can from a body that was not SSE, and otherwise say so
                // instead of handing the caller a silent empty turn.
                for chunk in recover_unstreamed_body(&unparsed) {
                    let _ = tx.send(chunk);
                }
            }
            let _ = tx.send(StreamChunk::Finish("stop".to_string()));
        });

        // Return the receiver immediately so the caller can start reading chunks
        // Metrics will be computed when the stream finishes — for now return placeholder
        Ok((
            rx,
            StreamMetrics {
                first_token_latency_ms: 0,
                total_latency_ms: 0,
                tokens_generated: 0,
                cached: false,
            },
        ))
    }

    #[allow(dead_code)]
    pub async fn list_models(&self) -> Result<Vec<String>> {
        // The Codex backend has no /models listing endpoint; models come
        // from config.
        if self.kind == ProviderKind::OpenAiCodex {
            return Ok(Vec::new());
        }
        let path = match self.kind {
            ProviderKind::Gemini => "/models",
            _ => "/models",
        };

        let mut models_req = self.build_request_builder(reqwest::Method::GET, path);
        if self.kind == ProviderKind::XaiOauth {
            models_req = self.apply_xai_auth(models_req, false).await?;
        }
        let response = models_req
            .send()
            .await
            .with_context(|| format!("Failed to list models from {}", self.base_url))?;

        let body: serde_json::Value = response
            .json()
            .await
            .with_context(|| "Failed to parse models response")?;

        let models = match self.kind {
            ProviderKind::OpenAiCodex => Vec::new(),
            ProviderKind::OpenAiCompatible | ProviderKind::Anthropic | ProviderKind::XaiOauth => {
                body.get("data")
                    .and_then(|d| d.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| {
                                m.get("id")
                                    .and_then(|id| id.as_str())
                                    .map(|s| s.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            }
            ProviderKind::Gemini => body
                .get("models")
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| {
                            m.get("name")
                                .and_then(|id| id.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };

        Ok(models)
    }
}

#[cfg(test)]
mod tests {
    fn spawn_mock_server(status_line: &'static str, body: &'static str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 65536];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status_line,
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        format!("http://{}/v1", addr)
    }

    fn test_request() -> ChatRequest {
        ChatRequest::new(
            "qwen-local".to_string(),
            vec![Message {
                role: "user".to_string(),
                content: format!("hi {}", uuid::Uuid::new_v4()),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            }],
            true,
        )
    }

    /// Serve one canned response per connection, written in separate
    /// `parts` with a pause between them so they arrive as separate reads.
    /// No Content-Length: the body ends when the connection closes.
    fn spawn_raw_server(content_type: &'static str, parts: Vec<Vec<u8>>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 65536];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
                    content_type
                );
                let _ = stream.write_all(head.as_bytes());
                for part in &parts {
                    let _ = stream.write_all(part);
                    let _ = stream.flush();
                    std::thread::sleep(std::time::Duration::from_millis(30));
                }
            }
        });
        format!("http://{}/v1", addr)
    }

    fn sse(lines: &[&str]) -> Vec<Vec<u8>> {
        lines
            .iter()
            .map(|l| format!("data: {}\n\n", l).into_bytes())
            .collect()
    }

    /// Run a realtime stream against `base` with the given provider kind
    /// and return (content, reasoning, tool calls, finish reasons).
    async fn collect_realtime(
        base: String,
        kind: ProviderKind,
    ) -> (String, String, Vec<(String, String, String)>, Vec<String>) {
        let p = Provider::new_without_cache(
            "xai".to_string(),
            base,
            "test-key".to_string(),
            kind,
            HashMap::new(),
        );
        let mut req = test_request();
        req.model = "grok-4.7".to_string();
        let (mut rx, _) = p.chat_stream_realtime(req).await.expect("stream starts");
        let (mut content, mut reasoning) = (String::new(), String::new());
        let (mut tools, mut finishes) = (Vec::new(), Vec::new());
        while let Some(chunk) = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("stream finished")
        {
            match chunk {
                StreamChunk::Content(c) => content.push_str(&c),
                StreamChunk::Reasoning(r) => reasoning.push_str(&r),
                StreamChunk::ToolCall {
                    id,
                    name,
                    arguments,
                } => tools.push((id, name, arguments)),
                StreamChunk::Finish(f) => finishes.push(f),
            }
        }
        (content, reasoning, tools, finishes)
    }

    #[tokio::test]
    async fn realtime_grok_stream_keeps_reasoning_and_content() {
        let base = spawn_raw_server(
            "text/event-stream",
            sse(&[
                r#"{"id":"1","object":"chat.completion.chunk","model":"grok-4.7","choices":[{"index":0,"delta":{"reasoning_content":"Greeting.","role":"assistant"}}]}"#,
                r#"{"id":"1","object":"chat.completion.chunk","model":"grok-4.7","choices":[{"index":0,"delta":{"content":"Hey!","role":"assistant"}}]}"#,
                r#"{"id":"1","object":"chat.completion.chunk","model":"grok-4.7","choices":[{"index":0,"delta":{"content":" What are we building?"},"finish_reason":"end_turn"}]}"#,
                "[DONE]",
            ]),
        );
        let (content, reasoning, tools, finishes) =
            collect_realtime(base, ProviderKind::XaiOauth).await;
        assert_eq!(content, "Hey! What are we building?");
        assert_eq!(reasoning, "Greeting.");
        assert!(tools.is_empty());
        assert_eq!(finishes.first().map(String::as_str), Some("end_turn"));
    }

    #[tokio::test]
    async fn realtime_error_event_in_2xx_stream_is_surfaced_not_empty() {
        // xAI-shaped error delivered as an SSE event on a 200 response.
        let base = spawn_raw_server(
            "text/event-stream",
            sse(&[
                r#"{"code":"Some resource has been exhausted","error":"Too many requests for grok-4.7, try again later"}"#,
            ]),
        );
        let (content, _, tools, finishes) = collect_realtime(base, ProviderKind::XaiOauth).await;
        assert!(content.starts_with("[API error:"), "{content}");
        assert!(content.contains("Too many requests"), "{content}");
        assert!(tools.is_empty());
        assert!(finishes.contains(&"error".to_string()));

        // OpenAI-shaped error object mid-stream, after some output.
        let base = spawn_raw_server(
            "text/event-stream",
            sse(&[
                r#"{"choices":[{"index":0,"delta":{"content":"Partial"}}]}"#,
                r#"{"error":{"message":"upstream overloaded","type":"server_error"}}"#,
            ]),
        );
        let (content, _, _, finishes) =
            collect_realtime(base, ProviderKind::OpenAiCompatible).await;
        assert!(
            content.starts_with("Partial[API error: upstream overloaded"),
            "{content}"
        );
        assert!(finishes.contains(&"error".to_string()));
    }

    #[tokio::test]
    async fn realtime_plain_json_error_body_with_2xx_is_surfaced() {
        let base = spawn_raw_server(
            "application/json",
            vec![br#"{"code":"permission-denied","error":"Your team has used all available credits"}"#.to_vec()],
        );
        let (content, _, _, finishes) = collect_realtime(base, ProviderKind::XaiOauth).await;
        assert!(content.contains("permission-denied"), "{content}");
        assert!(content.contains("used all available credits"), "{content}");
        assert!(finishes.contains(&"error".to_string()));
    }

    #[tokio::test]
    async fn realtime_non_streamed_completion_body_is_recovered() {
        // A server that ignores `stream: true` and answers with one JSON body.
        let base = spawn_raw_server(
            "application/json",
            vec![br#"{"object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Hey there.","tool_calls":[{"id":"call_1","type":"function","function":{"name":"list_dir","arguments":"{\"path\":\".\"}"}}]},"finish_reason":"tool_calls"}]}"#.to_vec()],
        );
        let (content, _, tools, _) = collect_realtime(base, ProviderKind::XaiOauth).await;
        assert_eq!(content, "Hey there.");
        assert_eq!(
            tools,
            vec![(
                "call_1".to_string(),
                "list_dir".to_string(),
                r#"{"path":"."}"#.to_string()
            )]
        );
    }

    #[tokio::test]
    async fn realtime_content_parts_array_and_unterminated_last_line() {
        let mut parts = sse(&[
            r#"{"choices":[{"index":0,"delta":{"content":[{"type":"text","text":"Hello"}]}}]}"#,
        ]);
        // Last event with no trailing newline before the connection closes.
        parts.push(br#"data: {"choices":[{"index":0,"delta":{"content":", world"},"finish_reason":"stop"}]}"#.to_vec());
        let base = spawn_raw_server("text/event-stream", parts);
        let (content, _, _, finishes) =
            collect_realtime(base, ProviderKind::OpenAiCompatible).await;
        assert_eq!(content, "Hello, world");
        assert_eq!(finishes.first().map(String::as_str), Some("stop"));
    }

    #[tokio::test]
    async fn realtime_utf8_split_across_network_chunks_is_intact() {
        let event = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Shields up 🛡 — ready\"}}]}\n\n";
        let bytes = event.as_bytes();
        let shield = event.find('🛡').unwrap();
        // Split in the middle of the 4-byte shield emoji.
        let parts = vec![bytes[..shield + 2].to_vec(), bytes[shield + 2..].to_vec()];
        let base = spawn_raw_server("text/event-stream", parts);
        let (content, _, _, _) = collect_realtime(base, ProviderKind::XaiOauth).await;
        assert_eq!(content, "Shields up 🛡 — ready");
    }

    #[tokio::test]
    async fn realtime_tool_calls_still_stream_normally() {
        let base = spawn_raw_server(
            "text/event-stream",
            sse(&[
                r#"{"choices":[{"index":0,"delta":{"content":"Let me look around.","tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"glob","arguments":"{\"pattern\":"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"**/*.md\"}"}}]},"finish_reason":"tool_calls"}]}"#,
                "[DONE]",
            ]),
        );
        let (content, _, tools, finishes) = collect_realtime(base, ProviderKind::XaiOauth).await;
        assert_eq!(content, "Let me look around.");
        assert_eq!(
            tools,
            vec![(
                "call_a".to_string(),
                "glob".to_string(),
                r#"{"pattern":"**/*.md"}"#.to_string()
            )]
        );
        assert_eq!(finishes.first().map(String::as_str), Some("tool_calls"));
    }

    #[test]
    fn stream_error_and_text_helpers() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"code":"permission-denied","error":"no credits"}"#).unwrap();
        assert_eq!(
            openai_stream_error(&v).as_deref(),
            Some("permission-denied: no credits")
        );
        let v: serde_json::Value =
            serde_json::from_str(r#"{"error":{"message":"bad","code":429}}"#).unwrap();
        assert_eq!(openai_stream_error(&v).as_deref(), Some("429: bad"));
        let v: serde_json::Value = serde_json::from_str(r#"{"error":null,"choices":[]}"#).unwrap();
        assert_eq!(openai_stream_error(&v), None);
        let parts: serde_json::Value = serde_json::from_str(
            r#"[{"type":"text","text":"a"},{"type":"output_text","text":"b"},{"type":"image_url"}]"#,
        )
        .unwrap();
        assert_eq!(openai_text(Some(&parts)).as_deref(), Some("ab"));
        assert_eq!(openai_text(Some(&serde_json::Value::Null)), None);
        // A body that is neither an error nor a completion is still reported.
        let out = recover_unstreamed_body("<html>502 Bad Gateway</html>");
        assert!(matches!(&out[0], StreamChunk::Content(c) if c.contains("502 Bad Gateway")));
        assert!(recover_unstreamed_body("   ").is_empty());
    }

    #[test]
    fn detects_unresolved_env_placeholders() {
        assert_eq!(
            unresolved_env_placeholder("${OPENAI_API_KEY}"),
            Some("OPENAI_API_KEY")
        );
        assert_eq!(unresolved_env_placeholder("ollama"), None);
        assert_eq!(unresolved_env_placeholder("${}"), None);
        assert_eq!(unresolved_env_placeholder("sk-${abc}"), None);
    }

    #[tokio::test]
    async fn unresolved_api_key_fails_before_sending() {
        let p = Provider::new_without_cache(
            "openai".to_string(),
            "http://127.0.0.1:9/v1".to_string(),
            "${OPENSHIELD_TEST_UNSET_KEY}".to_string(),
            ProviderKind::OpenAiCompatible,
            HashMap::new(),
        );
        let err = p.chat_stream(test_request()).await.err().unwrap();
        let msg = err.to_string();
        assert!(msg.contains("OPENSHIELD_TEST_UNSET_KEY"), "{msg}");
        assert!(msg.contains("openai"), "{msg}");
        let err = p.chat_stream_realtime(test_request()).await.err().unwrap();
        assert!(err.to_string().contains("OPENSHIELD_TEST_UNSET_KEY"));
    }

    #[test]
    fn context_overflow_errors_get_a_hint() {
        let body = r#"{"error":{"code":400,"message":"the request exceeds the available context size","type":"exceed_context_size_error"}}"#;
        assert!(is_context_overflow_error(body));
        assert!(is_context_overflow_error(
            r#"{"error":{"code":"context_length_exceeded"}}"#
        ));
        assert!(!is_context_overflow_error(r#"{"error":"invalid_api_key"}"#));
        let msg = format_api_error(reqwest::StatusCode::BAD_REQUEST, body);
        assert!(msg.contains("Context window exceeded"));
        assert!(msg.contains("num_ctx"));
    }

    #[tokio::test]
    async fn realtime_stream_returns_err_on_http_error() {
        let base = spawn_mock_server(
            "400 Bad Request",
            r#"{"error":{"message":"the request exceeds the available context size","type":"exceed_context_size_error"}}"#,
        );
        let p = Provider::new_without_cache(
            "ollama".to_string(),
            base,
            "ollama".to_string(),
            ProviderKind::OpenAiCompatible,
            HashMap::new(),
        );
        let err = p.chat_stream_realtime(test_request()).await.err().unwrap();
        assert!(err.to_string().contains("Context window exceeded"), "{err}");
        let err = p.chat_stream(test_request()).await.err().unwrap();
        assert!(err.to_string().contains("Context window exceeded"), "{err}");
    }

    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_build_chat_body_merges_multiple_system_messages() {
        // Strict chat templates (stock Qwen3 / llama.cpp minijinja) reject any
        // system message that is not messages[0] with a 500 Jinja error.
        // build_chat_body must merge all system messages into one leading
        // system message for OpenAI-compatible providers.
        let provider = Provider::new(
            "test".to_string(),
            "http://127.0.0.1:1/v1".to_string(),
            "test-key".to_string(),
            ProviderKind::OpenAiCompatible,
            std::collections::HashMap::new(),
        );
        let msg = |role: &str, content: &str| Message {
            role: role.to_string(),
            content: content.to_string(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        let request = ChatRequest::new(
            "m".to_string(),
            vec![
                msg("system", "PROMPT-A"),
                msg("user", "hello"),
                msg("assistant", "hi there"),
                msg("system", "PROMPT-B"),
                msg("user", "again"),
                msg("system", "PROMPT-C"),
            ],
            false,
        );
        let body = provider.build_chat_body(&request);
        let messages = body["messages"].as_array().unwrap();
        // Exactly one system message, at index 0, with all contents merged.
        let system_msgs: Vec<_> = messages
            .iter()
            .filter(|m| m["role"].as_str() == Some("system"))
            .collect();
        assert_eq!(
            system_msgs.len(),
            1,
            "must merge to a single system message"
        );
        assert_eq!(messages[0]["role"].as_str(), Some("system"));
        let merged = messages[0]["content"].as_str().unwrap();
        assert!(merged.contains("PROMPT-A"));
        assert!(merged.contains("PROMPT-B"));
        assert!(merged.contains("PROMPT-C"));
        // Non-system order preserved.
        let roles: Vec<_> = messages
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, vec!["system", "user", "assistant", "user"]);
    }

    #[test]
    fn test_build_chat_body_single_system_untouched() {
        let provider = Provider::new(
            "test".to_string(),
            "http://127.0.0.1:1/v1".to_string(),
            "test-key".to_string(),
            ProviderKind::OpenAiCompatible,
            std::collections::HashMap::new(),
        );
        let msg = |role: &str, content: &str| Message {
            role: role.to_string(),
            content: content.to_string(),
            images: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        let request = ChatRequest::new(
            "m".to_string(),
            vec![msg("system", "ONLY"), msg("user", "hi")],
            false,
        );
        let body = provider.build_chat_body(&request);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["content"].as_str(), Some("ONLY"));
    }

    #[test]
    fn test_chat_request_serialization() {
        let request = ChatRequest::new(
            "gpt-4".to_string(),
            vec![
                Message {
                    role: "system".to_string(),
                    content: "You are a helpful assistant".to_string(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                Message {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
            ],
            false,
        );

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("system"));
        assert!(json.contains("user"));
    }

    #[test]
    fn test_chat_response_deserialization() {
        let json = r#"{
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "Hello!"
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content, "Hello!");
        assert_eq!(response.usage.as_ref().unwrap().total_tokens, 15);
    }

    #[test]
    fn test_choice_creation() {
        let choice = Choice {
            message: Message {
                role: "assistant".to_string(),
                content: "Test".to_string(),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
            finish_reason: Some("stop".to_string()),
        };
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_usage_creation() {
        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn test_provider_new() {
        let provider = Provider::new(
            "test".to_string(),
            "https://api.test.com".to_string(),
            "test-key".to_string(),
            ProviderKind::OpenAiCompatible,
            HashMap::new(),
        );
        assert_eq!(provider.name, "test");
        assert_eq!(provider.base_url, "https://api.test.com");
        assert_eq!(provider.api_key, "test-key");
    }

    #[test]
    fn test_provider_build_openai_body() {
        let provider = Provider::new(
            "openai".to_string(),
            "https://api.openai.com/v1".to_string(),
            "key".to_string(),
            ProviderKind::OpenAiCompatible,
            HashMap::new(),
        );
        let request = ChatRequest::new(
            "gpt-4".to_string(),
            vec![Message {
                role: "user".to_string(),
                content: "hi".to_string(),
                images: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            }],
            false,
        );
        let body = provider.build_chat_body(&request);
        assert_eq!(body["model"], "gpt-4");
        assert!(!body["messages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_provider_build_anthropic_body() {
        let provider = Provider::new(
            "anthropic".to_string(),
            "https://api.anthropic.com/v1".to_string(),
            "key".to_string(),
            ProviderKind::Anthropic,
            HashMap::new(),
        );
        let request = ChatRequest::new(
            "claude-sonnet-4".to_string(),
            vec![
                Message {
                    role: "system".to_string(),
                    content: "Be helpful".to_string(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                Message {
                    role: "user".to_string(),
                    content: "hi".to_string(),
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
            ],
            false,
        );
        let body = provider.build_chat_body(&request);
        assert_eq!(body["model"], "claude-sonnet-4");
        assert_eq!(body["system"], "Be helpful");
        // System message should be filtered from messages array
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_provider_parse_anthropic_response() {
        let provider = Provider::new(
            "anthropic".to_string(),
            "https://api.anthropic.com/v1".to_string(),
            "key".to_string(),
            ProviderKind::Anthropic,
            HashMap::new(),
        );
        let json = r#"{
            "content": [{"type": "text", "text": "Hello!"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }"#;
        let response = provider.parse_chat_response(json).unwrap();
        assert_eq!(response.choices[0].message.content, "Hello!");
        assert_eq!(response.usage.as_ref().unwrap().total_tokens, 15);
    }
}
