use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
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
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
}

impl Provider {
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let client = reqwest::Client::new();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .with_context(|| format!("Failed to send request to {}", url))?;

        let status = response.status();
        let body = response.text().await
            .with_context(|| "Failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!("API error {}: {}", status, body);
        }

        let chat_response: ChatResponse = serde_json::from_str(&body)
            .with_context(|| format!("Failed to parse response: {}", body))?;

        Ok(chat_response)
    }

    pub async fn chat_stream(&self, request: ChatRequest) -> Result<Vec<String>> {
        let client = reqwest::Client::new();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": request.model,
                "messages": request.messages,
                "stream": true
            }))
            .send()
            .await
            .with_context(|| format!("Failed to send request to {}", url))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("API error {}: {}", status, body);
        }

        let mut stream = response.bytes_stream();
        let mut chunks = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            let text = String::from_utf8_lossy(&chunk);

            for line in text.lines() {
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        continue;
                    }
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(delta) = event
                            .get("choices")
                            .and_then(|c| c.get(0))
                            .and_then(|c| c.get("delta"))
                            .and_then(|d| d.get("content"))
                            .and_then(|c| c.as_str())
                        {
                            chunks.push(delta.to_string());
                        }
                    }
                }
            }
        }

        Ok(chunks)
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        let client = reqwest::Client::new();
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));

        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .with_context(|| format!("Failed to list models from {}", url))?;

        let body: serde_json::Value = response.json().await
            .with_context(|| "Failed to parse models response")?;

        let models = body.get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }
}
