//! Mock OpenAI-compatible provider. Returns canned replies so the harness
//! runs with no API keys and no network.

use axum::{
    Json, Router,
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

pub const MODEL: &str = "mock-1";

/// Build a reply from the last user message.
pub fn reply_for(body: &Value) -> String {
    let last = body["messages"]
        .as_array()
        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
        .map(|m| match &m["content"] {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        })
        .unwrap_or_default();
    let short: String = last.chars().take(200).collect();
    format!(
        "🛡 [mock provider] You said: \"{}\"\n\nThis is the OpenShield demo. Replies come from a built-in mock model, so no API key or network is needed. Point a real provider at OpenShield with `openshield setup` to get real answers.",
        short.trim()
    )
}

async fn models() -> Json<Value> {
    Json(
        json!({"object": "list", "data": [{"id": MODEL, "object": "model", "owned_by": "openshield-demo"}]}),
    )
}

async fn chat(Json(body): Json<Value>) -> Response {
    let text = reply_for(&body);
    let model = body["model"].as_str().unwrap_or(MODEL).to_string();
    let usage = json!({"prompt_tokens": 10, "completion_tokens": 40, "total_tokens": 50});
    if body["stream"].as_bool().unwrap_or(false) {
        let mut sse = String::new();
        let first = json!({"id": "mock", "object": "chat.completion.chunk", "model": model,
            "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": null}]});
        sse.push_str(&format!("data: {}\n\n", first));
        for word in text.split_inclusive(' ') {
            let c = json!({"id": "mock", "object": "chat.completion.chunk", "model": model,
                "choices": [{"index": 0, "delta": {"content": word}, "finish_reason": null}]});
            sse.push_str(&format!("data: {}\n\n", c));
        }
        let last = json!({"id": "mock", "object": "chat.completion.chunk", "model": model,
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}], "usage": usage});
        sse.push_str(&format!("data: {}\n\ndata: [DONE]\n\n", last));
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(sse))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    Json(json!({
        "id": "mock", "object": "chat.completion", "model": model,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
        "usage": usage
    }))
    .into_response()
}

pub fn router() -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat))
}

pub async fn serve(addr: &str) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router()).await?;
    Ok(())
}

/// Bind on an ephemeral loopback port and serve in the background.
pub async fn spawn_ephemeral() -> anyhow::Result<std::net::SocketAddr> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, router()).await;
    });
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_echoes_last_user_message() {
        let body = json!({"messages": [
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "hello shield"}
        ]});
        assert!(reply_for(&body).contains("hello shield"));
    }

    #[tokio::test]
    async fn serves_chat_completion() {
        let addr = spawn_ephemeral().await.unwrap();
        let provider = crate::providers::Provider::new_without_cache(
            "mock".into(),
            format!("http://{}/v1", addr),
            "demo".into(),
            crate::config::ProviderKind::OpenAiCompatible,
            Default::default(),
        );
        let req = crate::providers::ChatRequest::new(
            MODEL.into(),
            vec![crate::providers::Message::text("user", "ping")],
            false,
        );
        let resp = provider.chat(req).await.unwrap();
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v.to_string().contains("ping"));
    }
}
