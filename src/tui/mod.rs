use anyhow::Result;
use crate::config::Config;
use crate::memory::{MemoryStore, Message as MemoryMessage};
use crate::providers::{ChatRequest, Message, Provider};
use crate::tools::{find_tool, get_tools};
use chrono::Utc;
use std::io::{self, Write};
use uuid::Uuid;

pub async fn run(config: Config) -> Result<()> {
    let provider = Provider {
        name: "local".to_string(),
        base_url: config
            .providers
            .get("local")
            .map(|p| p.base_url.clone())
            .unwrap_or_else(|| "http://127.0.0.1:8080/v1".to_string()),
        api_key: "local".to_string(),
    };

    let memory = MemoryStore::new(&config.memory_db_path)?;
    let session_id = Uuid::new_v4().to_string();
    let model = config.default_model.clone();

    memory.create_session(&session_id, &model, "general")?;

    println!("🦈 OpenShark Session: {}", &session_id[..8]);
    println!("Model: {}", model);
    println!("Type 'help' for commands, 'exit' to quit.");
    println!();

    let mut messages: Vec<Message> = vec![Message {
        role: "system".to_string(),
        content: format!(
            "You are OpenShark, an AI coding assistant. You have access to tools:\n{}\n\
             When you need to use a tool, respond with: TOOL:tool_name args\n\
             Be concise and direct. Don't overthink.",
            get_tools()
                .iter()
                .map(|t| format!("- {}: {}", t.name(), t.description()))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }];

    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input == "exit" {
            println!("🦈 Session saved. Goodbye!");
            break;
        }

        if input == "help" {
            println!("Commands:");
            println!("  help     - Show this help");
            println!("  tools    - List available tools");
            println!("  history  - Show session history");
            println!("  exit     - End session");
            println!();
            println!("Tool usage: TOOL:tool_name args");
            continue;
        }

        if input == "tools" {
            println!("Available tools:");
            for tool in get_tools() {
                println!("  {} - {}", tool.name(), tool.description());
            }
            continue;
        }

        if input == "history" {
            let history = memory.get_session_messages(&session_id)?;
            for msg in history {
                println!("[{}] {}: {}", msg.created_at, msg.role, &msg.content[..msg.content.len().min(80)]);
            }
            continue;
        }

        // Save user message
        let user_msg = MemoryMessage {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            role: "user".to_string(),
            content: input.to_string(),
            created_at: Utc::now(),
            tokens_used: None,
        };
        memory.save_message(&user_msg)?;

        messages.push(Message {
            role: "user".to_string(),
            content: input.to_string(),
        });

        // Check if user is invoking a tool directly
        if input.starts_with("TOOL:") {
            let rest = &input[5..];
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() >= 1 {
                let tool_name = parts[0];
                let args = parts.get(1).unwrap_or(&"");

                if let Some(tool) = find_tool(tool_name) {
                    println!("🔧 Using tool: {}", tool_name);
                    match tool.execute(args) {
                        Ok(result) => {
                            println!("Result:\n{}", result);

                            // Save tool call
                            let tool_call = crate::memory::ToolCall {
                                id: Uuid::new_v4().to_string(),
                                session_id: session_id.clone(),
                                tool_name: tool_name.to_string(),
                                args: args.to_string(),
                                result: result.clone(),
                                success: true,
                                created_at: Utc::now(),
                            };
                            memory.save_tool_call(&tool_call)?;

                            // Add result to context
                            messages.push(Message {
                                role: "user".to_string(),
                                content: format!("Tool {} returned: {}", tool_name, result),
                            });
                        }
                        Err(e) => {
                            println!("❌ Tool error: {}", e);
                        }
                    }
                } else {
                    println!("❌ Unknown tool: {}", tool_name);
                }
                continue;
            }
        }

        // Call the model with streaming
        print!("🦈 ");
        io::stdout().flush()?;

        let request = ChatRequest {
            model: model.clone(),
            messages: messages.clone(),
            stream: true,
        };

        match provider.chat_stream(request).await {
            Ok(chunks) => {
                let mut full_content = String::new();
                for chunk in chunks {
                    print!("{}", chunk);
                    io::stdout().flush()?;
                    full_content.push_str(&chunk);
                }
                println!();

                // Check if model wants to use a tool
                if full_content.starts_with("TOOL:") {
                    println!("🔧 Model wants to use a tool");
                    let rest = &full_content[5..];
                    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                    if parts.len() >= 1 {
                        let tool_name = parts[0].trim();
                        let args = parts.get(1).unwrap_or(&"").trim();

                        if let Some(tool) = find_tool(tool_name) {
                            println!("  Tool: {}", tool_name);
                            println!("  Args: {}", args);
                            match tool.execute(args) {
                                Ok(result) => {
                                    println!("  Result: {}", &result[..result.len().min(200)]);

                                    let tool_call = crate::memory::ToolCall {
                                        id: Uuid::new_v4().to_string(),
                                        session_id: session_id.clone(),
                                        tool_name: tool_name.to_string(),
                                        args: args.to_string(),
                                        result: result.clone(),
                                        success: true,
                                        created_at: Utc::now(),
                                    };
                                    memory.save_tool_call(&tool_call)?;

                                    messages.push(Message {
                                        role: "assistant".to_string(),
                                        content: full_content.clone(),
                                    });
                                    messages.push(Message {
                                        role: "user".to_string(),
                                        content: format!("Tool result: {}", result),
                                    });

                                    // Get final response with streaming
                                    print!("🦈 ");
                                    io::stdout().flush()?;
                                    let follow_up = ChatRequest {
                                        model: model.clone(),
                                        messages: messages.clone(),
                                        stream: true,
                                    };
                                    if let Ok(resp_chunks) = provider.chat_stream(follow_up).await {
                                        let mut follow_content = String::new();
                                        for chunk in resp_chunks {
                                            print!("{}", chunk);
                                            io::stdout().flush()?;
                                            follow_content.push_str(&chunk);
                                        }
                                        println!();
                                        messages.push(Message {
                                            role: "assistant".to_string(),
                                            content: follow_content.clone(),
                                        });

                                        let assistant_msg = MemoryMessage {
                                            id: Uuid::new_v4().to_string(),
                                            session_id: session_id.clone(),
                                            role: "assistant".to_string(),
                                            content: follow_content,
                                            created_at: Utc::now(),
                                            tokens_used: None,
                                        };
                                        memory.save_message(&assistant_msg)?;
                                    }
                                }
                                Err(e) => {
                                    println!("  ❌ Error: {}", e);
                                }
                            }
                        } else {
                            println!("  ❌ Unknown tool: {}", tool_name);
                        }
                    }
                } else {
                    messages.push(Message {
                        role: "assistant".to_string(),
                        content: full_content.clone(),
                    });

                    let assistant_msg = MemoryMessage {
                        id: Uuid::new_v4().to_string(),
                        session_id: session_id.clone(),
                        role: "assistant".to_string(),
                        content: full_content.clone(),
                        created_at: Utc::now(),
                        tokens_used: None,
                    };
                    memory.save_message(&assistant_msg)?;
                }
            }
            Err(e) => {
                println!("❌ Error: {}", e);
            }
        }

        println!();
    }

    Ok(())
}
