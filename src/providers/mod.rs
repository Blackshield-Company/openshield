use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::time::Instant;

use crate::cache::{ResponseCache, compute_cache_key};
use crate::config::ProviderKind;

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
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(15))
            .http2_prior_knowledge()
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
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(15))
            .http2_prior_knowledge()
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
        }

        // Add custom headers from config
        for (key, val) in &self.headers {
            builder = builder.header(key, val);
        }

        builder
    }

    /// Convert messages to the provider's native format.
    fn build_chat_body(&self, request: &ChatRequest) -> serde_json::Value {
        match self.kind {
            ProviderKind::OpenAiCompatible => {
                let messages: Vec<_> = request
                    .messages
                    .iter()
                    .filter(|m| {
                        // Filter out empty assistant/tool messages — they violate API requirements.
                        // Messages with tool_calls but empty content are fine (content becomes null).
                        if (m.role == "assistant" || m.role == "tool")
                            && m.content.trim().is_empty()
                            && m.tool_calls.is_none()
                        {
                            return false;
                        }
                        true
                    })
                    .map(|m| {
                        // For assistant messages with tool_calls but no content,
                        // the API requires content to be null (not empty string)
                        let content = if m.role == "assistant"
                            && m.content.is_empty()
                            && m.tool_calls.is_some()
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
                            if !reasoning.is_empty() {
                                msg["reasoning_content"] = json!(reasoning);
                            }
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
        }
    }

    /// Parse a response from the provider's native format into our ChatResponse.
    /// Also extracts Kimi thinking content from `<think>...</think>` tags.
    fn parse_chat_response(&self, body: &str) -> Result<ChatResponse> {
        match self.kind {
            ProviderKind::OpenAiCompatible => {
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

                // Extract reasoning_content if present (Kimi thinking)
                if let Some(reasoning) = raw["choices"][0]["message"]["reasoning_content"].as_str()
                {
                    if !reasoning.is_empty() {
                        content = format!("<think>\n{}\n</think>\n\n{}", reasoning, content);
                    }
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
                            tool_calls: None,
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
        }
    }

    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let messages_json = serde_json::to_string(&request.messages)
            .with_context(|| "Failed to serialize messages for cache key")?;
        let cache_key = compute_cache_key(&request.model, &messages_json);

        if let Some(ref cache) = self.cache {
            if let Some(cached) = cache.get(&cache_key) {
                let chat_response = self.parse_chat_response(&cached.response)?;
                return Ok(chat_response);
            }
        }

        let body = self.build_chat_body(&request);
        let response = self
            .build_request_builder(reqwest::Method::POST, "/chat/completions")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to send request to {}", self.base_url))?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .with_context(|| "Failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!("API error {}: {}", status, body_text);
        }

        let chat_response = self.parse_chat_response(&body_text)?;

        if let Some(ref cache) = self.cache {
            let ttl_secs = if request.stream { 3600 } else { 86400 };
            let _ = cache.set(&cache_key, &body_text, ttl_secs);
        }

        Ok(chat_response)
    }

    pub async fn chat_stream(&self, request: ChatRequest) -> Result<(Vec<String>, StreamMetrics)> {
        let start_time = Instant::now();
        let body = self.build_chat_body(&request);
        let messages_json = serde_json::to_string(&body.get("messages").unwrap_or(&json!([])))
            .with_context(|| "Failed to serialize messages for cache key")?;
        let cache_key = compute_cache_key(&request.model, &messages_json);

        if let Some(ref cache) = self.cache {
            if let Some(cached) = cache.get(&cache_key) {
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
        }

        let body = self.build_chat_body(&request);
        let response = self
            .build_request_builder(reqwest::Method::POST, "/chat/completions")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to send request to {}", self.base_url))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            anyhow::bail!("API error {}: {}", status, body_text);
        }

        let mut stream = response.bytes_stream();
        let mut chunks = Vec::new();
        let mut first_token_time: Option<Instant> = None;
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
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
                            ProviderKind::OpenAiCompatible => {
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
                                if let Some(r) = reasoning {
                                    if !r.is_empty() {
                                        // Emit reasoning content wrapped in think tags so the TUI can display it
                                        chunks.push(format!("<think>{}</think>", r));
                                    }
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
                        };

                        if let Some(delta) = delta_content {
                            if first_token_time.is_none() {
                                first_token_time = Some(Instant::now());
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
        if let Some(ref cache) = self.cache {
            if let Some(cached) = cache.get(&cache_key) {
                if let Ok(chunks) = serde_json::from_str::<Vec<String>>(&cached.response) {
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
            }
        }

        let body = self.build_chat_body(&request);
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let kind = self.kind.clone();
        let headers = self.headers.clone();

        // Spawn the streaming work in a background task so the caller can start receiving immediately
        tokio::spawn(async move {
            let mut request_builder = client
                .request(
                    reqwest::Method::POST,
                    format!("{}{}", base_url.trim_end_matches('/'), "/chat/completions"),
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
            }
            for (key, val) in &headers {
                request_builder = request_builder.header(key, val);
            }

            let response = match request_builder.send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(StreamChunk::Content(format!("[stream error: {}]", e)));
                    return;
                }
            };

            let status = response.status();
            if !status.is_success() {
                let body_text = response.text().await.unwrap_or_default();
                let _ = tx.send(StreamChunk::Content(format!(
                    "[API error {}: {}]",
                    status, body_text
                )));
                return;
            }

            let mut stream = response.bytes_stream();
            let mut first_token_time: Option<Instant> = None;
            let mut buffer = String::new();
            // Accumulate tool call fragments across SSE chunks, keyed by index
            let mut pending_tool_calls: HashMap<u32, AccumulatedToolCall> = HashMap::new();

            let stream_timeout = std::time::Duration::from_secs(120);
            loop {
                let chunk = tokio::select! {
                    chunk_result = stream.next() => {
                        match chunk_result {
                            Some(Ok(c)) => c,
                            Some(Err(e)) => {
                                let _ = tx.send(StreamChunk::Content(format!("[stream read error: {}]", e)));
                                return;
                            }
                            None => break, // stream ended normally
                        }
                    }
                    _ = tokio::time::sleep(stream_timeout) => {
                        let _ = tx.send(StreamChunk::Content(
                            "[⏱ Stream timed out after 120s of inactivity]".to_string()
                        ));
                        break;
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
                            match kind {
                                ProviderKind::OpenAiCompatible => {
                                    let delta = event
                                        .get("choices")
                                        .and_then(|c| c.get(0))
                                        .and_then(|c| c.get("delta"));
                                    let content = delta
                                        .and_then(|d| d.get("content"))
                                        .and_then(|c| c.as_str());
                                    let reasoning = delta
                                        .and_then(|d| d.get("reasoning_content"))
                                        .and_then(|c| c.as_str());
                                    let tool_calls = delta
                                        .and_then(|d| d.get("tool_calls"))
                                        .and_then(|t| t.as_array());
                                    let finish_reason = event
                                        .get("choices")
                                        .and_then(|c| c.get(0))
                                        .and_then(|c| c.get("finish_reason"))
                                        .and_then(|f| f.as_str());

                                    // Emit finish reason if present
                                    if let Some(fr) = finish_reason {
                                        if fr == "tool_calls" {
                                            // Flush all accumulated tool calls
                                            for (_idx, tc) in pending_tool_calls.drain() {
                                                let _ = tx.send(StreamChunk::ToolCall {
                                                    id: tc.id,
                                                    name: tc.name,
                                                    arguments: tc.arguments,
                                                });
                                            }
                                            let _ = tx.send(StreamChunk::Finish(fr.to_string()));
                                        } else if fr == "stop" {
                                            let _ = tx.send(StreamChunk::Finish(fr.to_string()));
                                        }
                                    }

                                    // Accumulate tool call fragments by index
                                    if let Some(tcs) = tool_calls {
                                        for tc in tcs {
                                            let index = tc
                                                .get("index")
                                                .and_then(|i| i.as_u64())
                                                .unwrap_or(0)
                                                as u32;
                                            let entry =
                                                pending_tool_calls.entry(index).or_default();

                                            if let Some(id) = tc.get("id").and_then(|i| i.as_str())
                                            {
                                                if !id.is_empty() {
                                                    entry.id.push_str(id);
                                                }
                                            }
                                            if let Some(name) = tc
                                                .get("function")
                                                .and_then(|f| f.get("name"))
                                                .and_then(|n| n.as_str())
                                            {
                                                if !name.is_empty() {
                                                    entry.name.push_str(name);
                                                }
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

                                    if let Some(r) = reasoning {
                                        if !r.is_empty() {
                                            let _ = tx.send(StreamChunk::Reasoning(r.to_string()));
                                        }
                                    }
                                    if let Some(c) = content {
                                        if !c.is_empty() {
                                            if first_token_time.is_none() {
                                                first_token_time = Some(Instant::now());
                                            }
                                            let _ = tx.send(StreamChunk::Content(c.to_string()));
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
                                    if let Some(t) = text {
                                        if !t.is_empty() {
                                            if first_token_time.is_none() {
                                                first_token_time = Some(Instant::now());
                                            }
                                            let _ = tx.send(StreamChunk::Content(t.to_string()));
                                        }
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
                                    if let Some(t) = text {
                                        if !t.is_empty() {
                                            if first_token_time.is_none() {
                                                first_token_time = Some(Instant::now());
                                            }
                                            let _ = tx.send(StreamChunk::Content(t.to_string()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Stream ended normally — send Finish so the TUI knows we're done
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
        let path = match self.kind {
            ProviderKind::Gemini => "/models",
            _ => "/models",
        };

        let response = self
            .build_request_builder(reqwest::Method::GET, path)
            .send()
            .await
            .with_context(|| format!("Failed to list models from {}", self.base_url))?;

        let body: serde_json::Value = response
            .json()
            .await
            .with_context(|| "Failed to parse models response")?;

        let models = match self.kind {
            ProviderKind::OpenAiCompatible | ProviderKind::Anthropic => body
                .get("data")
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
                .unwrap_or_default(),
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
        assert!(body["messages"].as_array().unwrap().len() > 0);
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
