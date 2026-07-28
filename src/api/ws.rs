//! WebSocket handlers for streaming chat and agent execution.

use axum::extract::{
    State, WebSocketUpgrade,
    ws::{Message, WebSocket},
};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use super::AppState;

/// WS message from client.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Chat {
        message: String,
        #[serde(default)]
        model: Option<String>,
    },
    Agent {
        task: String,
        #[serde(default)]
        yolo: bool,
        #[serde(default = "default_max_turns")]
        max_turns: usize,
    },
    Ping,
}

fn default_max_turns() -> usize {
    50
}

/// Build chat messages with the agent's identity (soul) as the system
/// prompt. Without this the model falls back to its training identity
/// (e.g. "I'm Kimi") instead of the configured agent.
fn chat_messages(config: &crate::config::Config, message: String) -> Vec<crate::providers::Message> {
    let soul = crate::agent::soul::AgentSoul::from_config(config.agent.clone());
    let msg = |role: &str, content: String| crate::providers::Message {
        role: role.to_string(),
        content,
        images: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    vec![msg("system", soul.system_prompt()), msg("user", message)]
}

/// WS message to client.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Pong,
    Thinking {
        content: String,
    },
    Token {
        content: String,
    },
    ToolCall {
        name: String,
        args: String,
        turn: usize,
    },
    ToolResult {
        name: String,
        output: String,
        success: bool,
        turn: usize,
    },
    Error {
        message: String,
    },
    Complete {
        summary: String,
        total_turns: usize,
        duration_secs: u64,
    },
}

/// GET /ws/v1/chat — upgrade to WebSocket for streaming chat.
pub async fn ws_chat(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_chat_ws(socket, state))
}

/// GET /ws/v1/agent — upgrade to WebSocket for streaming agent tasks.
pub async fn ws_agent(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_agent_ws(socket, state))
}

fn build_provider(
    config: &crate::config::Config,
    model: &str,
) -> Option<(crate::providers::Provider, String)> {
    // Route to the provider that actually owns the requested model.
    // Fall back to the first provider for models not listed in config
    // (e.g. proxies that accept arbitrary model names).
    let (name, pc) = config.find_provider_for_model(model).or_else(|| {
        config
            .providers
            .iter()
            .next()
            .map(|(n, p)| (n.clone(), p.clone()))
    })?;
    let provider = crate::providers::Provider::new(
        name.clone(),
        pc.base_url.clone(),
        pc.api_key.clone(),
        pc.kind.clone(),
        pc.headers.clone(),
    );
    Some((provider, name.clone()))
}

async fn handle_chat_ws(mut socket: WebSocket, state: AppState) {
    while let Some(Ok(msg)) = socket.recv().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = send_json(
                    &mut socket,
                    &ServerMessage::Error {
                        message: format!("Invalid message: {}", e),
                    },
                )
                .await;
                continue;
            }
        };

        match client_msg {
            ClientMessage::Ping => {
                let _ = send_json(&mut socket, &ServerMessage::Pong).await;
            }
            ClientMessage::Chat { message, model } => {
                let config = match state.reload_config() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = send_json(
                            &mut socket,
                            &ServerMessage::Error {
                                message: format!("Config error: {}", e),
                            },
                        )
                        .await;
                        continue;
                    }
                };
                let model = model.unwrap_or_else(|| config.default_model.clone());

                let (provider, _) = match build_provider(&config, &model) {
                    Some(p) => p,
                    None => {
                        let _ = send_json(
                            &mut socket,
                            &ServerMessage::Error {
                                message: "No providers configured".to_string(),
                            },
                        )
                        .await;
                        continue;
                    }
                };

                let request = crate::providers::ChatRequest::new(
                    model,
                    chat_messages(&config, message),
                    true, // stream
                );

                // chat_stream returns (Vec<String>, StreamMetrics) — collects all chunks
                match provider.chat_stream(request).await {
                    Ok((chunks, _metrics)) => {
                        for chunk in &chunks {
                            let _ = send_json(
                                &mut socket,
                                &ServerMessage::Token {
                                    content: chunk.clone(),
                                },
                            )
                            .await;
                        }
                        let full = chunks.join("");
                        let _ = send_json(
                            &mut socket,
                            &ServerMessage::Complete {
                                summary: full,
                                total_turns: 1,
                                duration_secs: 0,
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        let _ = send_json(
                            &mut socket,
                            &ServerMessage::Error {
                                message: format!("Chat failed: {}", e),
                            },
                        )
                        .await;
                    }
                }
            }
            ClientMessage::Agent { .. } => {
                let _ = send_json(
                    &mut socket,
                    &ServerMessage::Error {
                        message: "Use /ws/v1/agent for agent tasks".to_string(),
                    },
                )
                .await;
            }
        }
    }
}

async fn handle_agent_ws(mut socket: WebSocket, state: AppState) {
    while let Some(Ok(msg)) = socket.recv().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = send_json(
                    &mut socket,
                    &ServerMessage::Error {
                        message: format!("Invalid message: {}", e),
                    },
                )
                .await;
                continue;
            }
        };

        match client_msg {
            ClientMessage::Ping => {
                let _ = send_json(&mut socket, &ServerMessage::Pong).await;
            }
            ClientMessage::Agent {
                task,
                yolo,
                max_turns,
            } => {
                let config = match state.reload_config() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = send_json(
                            &mut socket,
                            &ServerMessage::Error {
                                message: format!("Config error: {}", e),
                            },
                        )
                        .await;
                        continue;
                    }
                };
                let model = config.default_model.clone();

                let (provider, _) = match build_provider(&config, &model) {
                    Some(p) => p,
                    None => {
                        let _ = send_json(
                            &mut socket,
                            &ServerMessage::Error {
                                message: "No providers configured".to_string(),
                            },
                        )
                        .await;
                        continue;
                    }
                };

                let headless_config = crate::headless::HeadlessConfig {
                    task: task.clone(),
                    yolo,
                    autonomous: false,
                    json: false,
                    timeout_secs: 300,
                    max_turns,
                    model: None,
                    output_file: None,
                };

                let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
                let security = match crate::security::SecurityEngine::new(
                    crate::security::SecurityConfig::load().unwrap_or_default()
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = send_json(
                            &mut socket,
                            &ServerMessage::Error {
                                message: format!("Security engine failed: {}", e),
                            },
                        )
                        .await;
                        continue;
                    }
                };

                // Spawn headless agent in background
                tokio::spawn(async move {
                    let result = crate::headless::run_headless(
                        headless_config,
                        provider,
                        model,
                        security,
                        Some(event_tx),
                    )
                    .await;
                    let _ = result;
                });

                // Forward events to WebSocket client
                while let Some(event) = event_rx.recv().await {
                    let server_msg = match event {
                        crate::headless::HeadlessEvent::Start { .. } => ServerMessage::Thinking {
                            content: "Agent started".to_string(),
                        },
                        crate::headless::HeadlessEvent::Thought { content, .. } => {
                            ServerMessage::Thinking { content }
                        }
                        crate::headless::HeadlessEvent::ToolCall {
                            name, args, turn, ..
                        } => ServerMessage::ToolCall { name, args, turn },
                        crate::headless::HeadlessEvent::ToolResult {
                            name,
                            output,
                            success,
                            turn,
                            ..
                        } => ServerMessage::ToolResult {
                            name,
                            output,
                            success,
                            turn,
                        },
                        crate::headless::HeadlessEvent::Error { message, .. } => {
                            ServerMessage::Error { message }
                        }
                        crate::headless::HeadlessEvent::Complete {
                            summary,
                            total_turns,
                            duration_secs,
                            ..
                        } => {
                            let msg = ServerMessage::Complete {
                                summary,
                                total_turns,
                                duration_secs,
                            };
                            let _ = send_json(&mut socket, &msg).await;
                            break;
                        }
                    };

                    if send_json(&mut socket, &server_msg).await.is_err() {
                        break;
                    }
                }
            }
            ClientMessage::Chat { .. } => {
                let _ = send_json(
                    &mut socket,
                    &ServerMessage::Error {
                        message: "Use /ws/v1/chat for chat messages".to_string(),
                    },
                )
                .await;
            }
        }
    }
}

/// Send a JSON-encoded server message over the WebSocket.
async fn send_json(socket: &mut WebSocket, msg: &ServerMessage) -> Result<(), ()> {
    match serde_json::to_string(msg) {
        Ok(json) => socket
            .send(Message::Text(json.into()))
            .await
            .map_err(|_| ()),
        Err(_) => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_messages_include_identity_system_prompt() {
        let config = crate::config::Config::default();
        let msgs = chat_messages(&config, "hello".to_string());
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.contains(&config.agent.display_name));
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[1].content, "hello");
    }

    #[test]
    fn test_client_message_parse_chat() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"chat","message":"hello"}"#).unwrap();
        match msg {
            ClientMessage::Chat { message, .. } => assert_eq!(message, "hello"),
            _ => panic!("Expected Chat"),
        }
    }

    #[test]
    fn test_client_message_parse_agent() {
        let msg: ClientMessage = serde_json::from_str(
            r#"{"type":"agent","task":"fix tests","yolo":true,"max_turns":10}"#,
        )
        .unwrap();
        match msg {
            ClientMessage::Agent {
                task,
                yolo,
                max_turns,
            } => {
                assert_eq!(task, "fix tests");
                assert!(yolo);
                assert_eq!(max_turns, 10);
            }
            _ => panic!("Expected Agent"),
        }
    }

    #[test]
    fn test_client_message_parse_ping() {
        let msg: ClientMessage = serde_json::from_str(r#"{"type":"ping"}"#).unwrap();
        assert!(matches!(msg, ClientMessage::Ping));
    }

    #[test]
    fn test_server_message_serialize_pong() {
        let msg = ServerMessage::Pong;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("pong"));
    }

    #[test]
    fn test_server_message_serialize_token() {
        let msg = ServerMessage::Token {
            content: "hello".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("token"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_server_message_serialize_complete() {
        let msg = ServerMessage::Complete {
            summary: "done".to_string(),
            total_turns: 5,
            duration_secs: 30,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("complete"));
    }

    #[test]
    fn test_build_provider_routes_to_model_owner() {
        use crate::config::{Config, ModelConfig, ProviderConfig, ProviderKind};
        use std::collections::HashMap;

        let model = |name: &str| ModelConfig {
            name: name.to_string(),
            context_length: 128000,
            cost_per_1k_input: 0.0,
            cost_per_1k_output: 0.0,
            capabilities: vec![],
        };
        let provider = |url: &str, models| ProviderConfig {
            base_url: url.to_string(),
            api_key: "test".to_string(),
            models,
            kind: ProviderKind::OpenAiCompatible,
            headers: HashMap::new(),
            env_file: None,
        };

        let mut config = Config::default();
        config.providers.clear();
        config.providers.insert(
            "kimi".to_string(),
            provider("https://api.kimi.com/coding/v1", vec![model("k3")]),
        );
        config.providers.insert(
            "llama-swap".to_string(),
            provider("http://127.0.0.1:8080/v1", vec![model("local-fast")]),
        );

        // Regression: default_model owned by kimi must route to kimi even
        // when HashMap order would surface llama-swap first.
        let (p, name) = build_provider(&config, "k3").unwrap();
        assert_eq!(name, "kimi");
        assert_eq!(p.base_url, "https://api.kimi.com/coding/v1");

        let (_, name) = build_provider(&config, "local-fast").unwrap();
        assert_eq!(name, "llama-swap");

        // Unlisted model → first-provider fallback, never a panic.
        let (_, name) = build_provider(&config, "unlisted-model").unwrap();
        assert!(name == "kimi" || name == "llama-swap");
    }
}
