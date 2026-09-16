//! WebSocket handlers for streaming chat and agent execution.

use axum::extract::{
    State, WebSocketUpgrade,
    ws::{Message, WebSocket},
};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::AppState;

/// WS message from client. The inbound chat frame is
/// `{type: "chat", session, message}` (design doc §4); `session` is accepted
/// under its legacy `session_id` spelling too.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Chat {
        message: String,
        #[serde(default, alias = "session_id")]
        session: Option<String>,
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

/// Typed WS envelope for /ws/v1/chat, per design doc §4:
/// `{ "type": "token|tool_start|tool_result|approval_req|approval_res|turn_done|error", "session": "...", "data": {...} }`
/// The harness event system is serialized directly into this shape;
/// approval_req / approval_res arrive via the approvals broadcast (M3) and
/// are forwarded by the same send path.
#[derive(Debug, Serialize)]
struct WsEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    session: String,
    data: serde_json::Value,
}

impl WsEvent {
    fn new(event_type: &'static str, session: &str, data: serde_json::Value) -> Self {
        Self {
            event_type,
            session: session.to_string(),
            data,
        }
    }
}

/// What to do with a harness event on the WS chat stream.
enum Envelope {
    /// Emit this envelope to the client.
    Emit(WsEvent),
    /// The turn is finished — return to reading client messages.
    EndTurn,
    /// Internal event with no envelope equivalent — skip.
    Drop,
}

/// Map a harness engine event to its typed WS envelope (design doc §4).
fn harness_event_envelope(session: &str, event: crate::harness::HarnessEvent) -> Envelope {
    use crate::harness::HarnessEvent as HE;
    match event {
        HE::Start | HE::SystemMessage(_) => Envelope::Drop,
        HE::Chunk(text) => Envelope::Emit(WsEvent::new("token", session, json!({ "text": text }))),
        HE::ReasoningChunk(text) => Envelope::Emit(WsEvent::new(
            "token",
            session,
            json!({ "text": text, "reasoning": true }),
        )),
        HE::ToolCall {
            id,
            name,
            arguments,
        } => Envelope::Emit(WsEvent::new(
            "tool_start",
            session,
            json!({
                "id": id,
                "name": name,
                "args": parse_tool_args(&arguments),
            }),
        )),
        HE::ToolResult {
            tool_call_id,
            name,
            result,
            success,
            ..
        } => Envelope::Emit(WsEvent::new(
            "tool_result",
            session,
            json!({
                "id": tool_call_id,
                "name": name,
                "ok": success,
                "output": result,
            }),
        )),
        HE::AssistantComplete { metrics, .. } => Envelope::Emit(WsEvent::new(
            "turn_done",
            session,
            json!({
                "tokens": metrics.tokens_generated,
                "ms": metrics.total_latency_ms,
            }),
        )),
        // Post-tool-loop content arrives as a follow-up — same stream as tokens.
        HE::FollowUp(text) => {
            Envelope::Emit(WsEvent::new("token", session, json!({ "text": text })))
        }
        HE::MultiModelResponse { .. } => Envelope::Drop,
        HE::Error(e) => Envelope::Emit(WsEvent::new("error", session, json!({ "message": e }))),
        HE::Done => Envelope::EndTurn,
    }
}

/// Tool arguments arrive as a JSON string from the provider; parse them into
/// a real object for the envelope, falling back to the raw string.
fn parse_tool_args(arguments: &str) -> serde_json::Value {
    serde_json::from_str(arguments)
        .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()))
}

/// Run one chat turn through the harness engine, streaming events over `h_tx`.
///
/// The engine and rusqlite's `MemoryStore` are non-Send, so this runs on a
/// blocking thread with its own current-thread runtime (the headless-mode
/// pattern). Any startup or turn error is surfaced to the client as a final
/// `error` event before the channel closes.
async fn run_harness_turn(
    config: crate::config::Config,
    model: String,
    session_id: String,
    message: String,
    h_tx: mpsc::UnboundedSender<crate::harness::HarnessEvent>,
    approvals: Arc<crate::api::approvals::ApprovalsState>,
) {
    let err_tx = h_tx.clone();
    let result = async {
        let memory = crate::memory::MemoryStore::new(&config.memory_db_path)?;
        let history = super::handlers::session_history_messages(&memory, &session_id, 40);
        let security = crate::security::SecurityEngine::new(
            crate::security::SecurityConfig::load().unwrap_or_default(),
        )?;
        let harness_config = crate::harness::HarnessConfig {
            primary_model: model,
            max_tool_loops: 10,
            // The M3 human gate is installed on the engine below; this flag
            // keeps its legacy meaning (no interactive surface → don't block).
            require_tool_approval: false,
            multi_model_enabled: false,
            memory_context_limit: 5,
            skills_enabled: true,
            secondary_models: Vec::new(),
        };
        let mut engine = crate::harness::HarnessEngine::new_with_history_and_security(
            harness_config,
            config,
            memory,
            session_id.clone(),
            history,
            security,
        )?;
        // M3: High/Critical tool calls hold for a human when a desktop/pocket
        // client is connected to /ws/v1/chat (approval_req → POST
        // /api/v1/approvals/:id → resume, 120s timeout denies). The gate
        // short-circuits to allow when no client is connected, so behavior
        // without a GUI is unchanged.
        engine.set_approval_gate(Some(crate::api::approvals::make_gate(
            approvals,
            session_id,
        )));
        engine.run_turn_streaming(&message, h_tx, true).await?;
        anyhow::Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = err_tx.send(crate::harness::HarnessEvent::Error(e.to_string()));
    }
}

/// WS message to client (agent stream — /ws/v1/agent).
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Pong,
    Thinking {
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
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
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
    // M3: connected-client count decides whether the High/Critical human
    // gate engages, and approval_req / approval_res events are forwarded to
    // every chat client (GUI dispatches by session + id).
    state.approvals.client_connected();
    let mut approval_events = state.approvals.subscribe();

    'conn: loop {
        tokio::select! {
            msg = socket.recv() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | None => break 'conn,
                    Some(Ok(_)) => continue,
                    Some(Err(_)) => break 'conn,
                };

                let client_msg: ClientMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = send_json(
                            &mut socket,
                            &WsEvent::new(
                                "error",
                                "",
                                json!({ "message": format!("Invalid message: {}", e) }),
                            ),
                        )
                        .await;
                        continue;
                    }
                };

                match client_msg {
                    ClientMessage::Ping => {
                        let _ = send_json(&mut socket, &WsEvent::new("pong", "", json!({}))).await;
                    }
                    ClientMessage::Chat {
                        message,
                        session,
                        model,
                    } => {
                        let config = match state.reload_config() {
                            Ok(c) => c,
                            Err(e) => {
                                let _ = send_json(
                                    &mut socket,
                                    &WsEvent::new(
                                        "error",
                                        "",
                                        json!({ "message": format!("Config error: {}", e) }),
                                    ),
                                )
                                .await;
                                continue;
                            }
                        };
                        if config.providers.is_empty() {
                            let _ = send_json(
                                &mut socket,
                                &WsEvent::new("error", "", json!({ "message": "No providers configured" })),
                            )
                            .await;
                            continue;
                        }
                        let model = model.unwrap_or_else(|| config.default_model.clone());
                        let session_id = session.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                        // Harness engine + rusqlite MemoryStore are non-Send: the turn
                        // runs on a blocking thread with a local runtime, streaming
                        // HarnessEvents back over an unbounded channel.
                        let (h_tx, mut h_rx) =
                            mpsc::unbounded_channel::<crate::harness::HarnessEvent>();
                        let session_for_engine = session_id.clone();
                        let approvals = state.approvals.clone();
                        tokio::task::spawn_blocking(move || {
                            let rt = match tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                            {
                                Ok(rt) => rt,
                                Err(_) => return,
                            };
                            rt.block_on(run_harness_turn(
                                config,
                                model,
                                session_for_engine,
                                message,
                                h_tx,
                                approvals,
                            ));
                        });

                        // Serialize harness events into the typed envelope (§4),
                        // interleaved with approval_req / approval_res from the
                        // broadcast so a client mid-turn sees its own requests.
                        'turn: loop {
                            tokio::select! {
                                event = h_rx.recv() => {
                                    match event {
                                        Some(event) => match harness_event_envelope(&session_id, event) {
                                            Envelope::Emit(ev) => {
                                                if send_json(&mut socket, &ev).await.is_err() {
                                                    break 'turn;
                                                }
                                            }
                                            Envelope::EndTurn => break 'turn,
                                            Envelope::Drop => {}
                                        },
                                        None => break 'turn,
                                    }
                                }
                                ev = approval_events.recv() => {
                                    if let Ok(ev) = ev {
                                        if send_json(&mut socket, &ev).await.is_err() {
                                            break 'turn;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    ClientMessage::Agent { .. } => {
                        let _ = send_json(
                            &mut socket,
                            &WsEvent::new(
                                "error",
                                "",
                                json!({ "message": "Use /ws/v1/agent for agent tasks" }),
                            ),
                        )
                        .await;
                    }
                }
            }
            ev = approval_events.recv() => {
                // Approval events broadcast while this client is idle
                // (between turns). Mid-turn forwarding happens above.
                if let Ok(ev) = ev {
                    if send_json(&mut socket, &ev).await.is_err() {
                        break 'conn;
                    }
                }
            }
        }
    }

    state.approvals.client_disconnected();
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
                    crate::security::SecurityConfig::load().unwrap_or_default(),
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
                                session_id: None,
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

/// Send a JSON-encoded message over the WebSocket.
async fn send_json(socket: &mut WebSocket, msg: &impl Serialize) -> Result<(), ()> {
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
    fn test_client_message_parse_chat() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"chat","message":"hello"}"#).unwrap();
        match msg {
            ClientMessage::Chat { message, .. } => assert_eq!(message, "hello"),
            _ => panic!("Expected Chat"),
        }
    }

    #[test]
    fn test_client_message_chat_frame_shape() {
        // Design doc §4 inbound frame: {type:"chat", session, message}.
        let msg: ClientMessage = serde_json::from_str(
            r#"{"type":"chat","session":"sess-1","message":"hi","model":"k3"}"#,
        )
        .unwrap();
        match msg {
            ClientMessage::Chat {
                message,
                session,
                model,
            } => {
                assert_eq!(message, "hi");
                assert_eq!(session.as_deref(), Some("sess-1"));
                assert_eq!(model.as_deref(), Some("k3"));
            }
            _ => panic!("Expected Chat"),
        }

        // Legacy `session_id` spelling still accepted.
        let msg: ClientMessage =
            serde_json::from_str(r#"{"type":"chat","session_id":"sess-2","message":"hi"}"#)
                .unwrap();
        match msg {
            ClientMessage::Chat { session, .. } => {
                assert_eq!(session.as_deref(), Some("sess-2"))
            }
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
    fn test_ws_event_envelope_shape() {
        let ev = WsEvent::new("token", "sess-1", json!({ "text": "hi" }));
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "token");
        assert_eq!(json["session"], "sess-1");
        assert_eq!(json["data"]["text"], "hi");
    }

    #[test]
    fn test_server_message_serialize_pong() {
        let msg = ServerMessage::Pong;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("pong"));
    }

    #[test]
    fn test_server_message_serialize_complete() {
        let msg = ServerMessage::Complete {
            summary: "done".to_string(),
            total_turns: 5,
            duration_secs: 30,
            session_id: None,
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

    #[test]
    fn test_harness_event_envelope_types() {
        use crate::harness::HarnessEvent as HE;
        use crate::providers::{StreamMetrics, ToolCallRequest};

        let metrics = StreamMetrics {
            first_token_latency_ms: 10,
            total_latency_ms: 9000,
            tokens_generated: 1234,
            cached: false,
        };

        let cases: Vec<(HE, &str)> = vec![
            (HE::Chunk("hi".into()), "token"),
            (
                HE::ToolCall {
                    id: "1".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path":"x"}"#.into(),
                },
                "tool_start",
            ),
            (
                HE::ToolResult {
                    tool_call_id: "1".into(),
                    name: "read_file".into(),
                    args: "{}".into(),
                    result: "ok".into(),
                    success: true,
                },
                "tool_result",
            ),
            (
                HE::AssistantComplete {
                    content: "done".into(),
                    reasoning: None,
                    tool_calls: Vec::<ToolCallRequest>::new(),
                    metrics,
                    finish_reason: Some("stop".into()),
                },
                "turn_done",
            ),
            (HE::Error("boom".into()), "error"),
        ];

        for (event, expected_type) in cases {
            match harness_event_envelope("sess-1", event) {
                Envelope::Emit(ev) => {
                    let json = serde_json::to_value(&ev).unwrap();
                    assert_eq!(json["type"], expected_type);
                    assert_eq!(json["session"], "sess-1");
                }
                _ => panic!("expected Emit for {}", expected_type),
            }
        }

        assert!(matches!(
            harness_event_envelope("s", HE::Done),
            Envelope::EndTurn
        ));
        assert!(matches!(
            harness_event_envelope("s", HE::Start),
            Envelope::Drop
        ));
    }

    #[test]
    fn test_parse_tool_args_falls_back_to_raw_string() {
        assert_eq!(parse_tool_args(r#"{"a":1}"#), json!({ "a": 1 }));
        assert_eq!(
            parse_tool_args("not json"),
            serde_json::Value::String("not json".into())
        );
    }
}
