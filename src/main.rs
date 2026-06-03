use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::info;

/// The current version of OpenShark.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod agent;
mod cache;
mod capabilities;
mod config;
mod diff;
mod doctor;
mod evolution;
mod gateway;
mod headless;
mod integrations;
mod image_utils;
mod json_output;
mod linting;
mod lsp;
mod mcp;
mod mcp_server;
mod memory;
mod plugins;
mod providers;
mod repo_map;
mod sandbox;
mod router;
mod security;
mod self_correction;
mod self_improve;
mod session;
mod skills;
mod slash_commands;
mod swarm;
mod tools;
mod tui;
mod watch;

use crate::tools::Tool;
use config::Config;

/// Parse embedded TOOL: lines from CLI chat responses.
/// Handles both raw args and JSON-like `key="value"` formats.
fn parse_embedded_tools_cli(text: &str) -> Vec<(String, String)> {
    let mut tools = Vec::new();
    // Use regex to find TOOL: or TOOL. anywhere in the text (not just start of line)
    let re = match regex::Regex::new(r"TOOL[:\.]\s*(\S+)(?:\s+(.*))?$") {
        Ok(r) => r,
        Err(_) => return tools,
    };
    for cap in re.captures_iter(text) {
        let tool_name = cap
            .get(1)
            .map(|m| m.as_str().trim())
            .unwrap_or("")
            .to_string();
        let args = cap
            .get(2)
            .map(|m| m.as_str().trim())
            .unwrap_or("")
            .to_string();
        if tool_name.is_empty() {
            continue;
        }
        // Handle JSON-like `command="value"` format by extracting the value
        let args = if args.starts_with("command=\"") && args.ends_with("\"") {
            args[9..args.len() - 1].to_string()
        } else {
            args
        };
        tools.push((tool_name, args));
    }
    tools
}

#[derive(Parser)]
#[command(name = "openshark")]
#[command(about = "🦈 The harness that learns. The agent that decides.")]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Tui,
    Setup,
    Stats,
    Memory {
        #[arg(default_value = "")]
        query: String,
        #[arg(short, long, default_value_t = false)]
        recent: bool,
        #[arg(short, long, default_value_t = false)]
        semantic: bool,
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    Route,
    Learn,
    Agent {
        #[arg(default_value = "")]
        task: String,
    },
    Test {
        #[arg(default_value = "run")]
        cmd: String,
        #[arg(default_value = ".")]
        path: String,
    },
    Models,
    Chat {
        #[arg(default_value = "")]
        message: String,
        #[arg(short, long)]
        model: Option<String>,
        /// Attach a file to the conversation context
        #[arg(short, long)]
        file: Option<String>,
    },
    Config,
    Security {
        #[arg(default_value = "status")]
        cmd: String,
        #[arg(default_value = "")]
        arg: String,
    },
    Mcp {
        #[arg(default_value = "status")]
        cmd: String,
    },
    Swarm {
        #[arg(default_value = "status")]
        cmd: String,
        #[arg(default_value = "")]
        prompt: String,
    },
    Tools {
        #[arg(default_value = "list")]
        cmd: String,
    },
    Doctor {
        #[arg(short, long, default_value_t = false)]
        fix: bool,
        #[arg(short, long, default_value = "")]
        component: String,
    },
    Plugins {
        #[arg(default_value = "list")]
        cmd: String,
        #[arg(default_value = "")]
        name: String,
    },
    /// Delegate a task to an external agent (claw, opencode, claude)
    Delegate {
        #[arg(default_value = "")]
        agent: String,
        #[arg(default_value = "")]
        task: String,
    },
    /// Hermes bridge commands
    Hermes {
        #[arg(default_value = "status")]
        cmd: String,
    },
    /// Run in headless / CI-CD mode (non-interactive)
    Headless {
        /// Task to execute
        #[arg(default_value = "")]
        task: String,
        /// Auto-approve all tool calls
        #[arg(short, long, default_value_t = false)]
        yolo: bool,
        /// Output NDJSON for structured consumption
        #[arg(short, long, default_value_t = false)]
        json: bool,
        /// Max seconds to run
        #[arg(short, long, default_value_t = 300)]
        timeout: u64,
        /// Max agent turns
        #[arg(short = 'n', long, default_value_t = 50)]
        max_turns: usize,
        /// Override model for this run
        #[arg(short, long)]
        model: Option<String>,
        /// Write output to file
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Build a repo map of the codebase
    RepoMap {
        #[arg(default_value = ".")]
        path: String,
    },
    /// Run linter on project
    Lint {
        #[arg(default_value = ".")]
        path: String,
    },
    /// Run as an MCP server
    McpServer,
    /// Show git diff of AI-made changes
    Diff,
    /// Export session to markdown
    Export {
        #[arg(default_value = "session")]
        name: String,
    },
    /// Watch files and auto-run commands
    Watch {
        #[arg(default_value = ".")]
        path: String,
        #[arg(short, long, default_value = "test")]
        cmd: String,
        #[arg(short, long, default_value_t = 1000)]
        debounce: u64,
    },
    /// Use a config profile
    Profile {
        #[arg(default_value = "")]
        name: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Config::load_or_default()?;

    match cli.command {
        Some(Commands::Tui) | None => {
            info!("Starting OpenShark TUI");

            #[cfg(feature = "discord")]
            // Spawn Discord gateway if enabled
            if config.gateway.discord.enabled {
                info!("Discord gateway enabled — spawning bot");
                let discord_config = config.clone();
                std::thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::error!("Failed to create Discord runtime: {}", e);
                            return;
                        }
                    };
                    rt.block_on(async {
                        let mut event_rx =
                            crate::gateway::discord::spawn_bot(discord_config.clone());
                        let mut router = match crate::gateway::message_router::MessageRouter::new(
                            discord_config,
                        ) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::error!("Failed to create message router: {}", e);
                                return;
                            }
                        };

                        while let Some(event) = event_rx.recv().await {
                            router.handle_event(event).await;
                        }
                    });
                });
            }

            #[cfg(feature = "telegram")]
            // Spawn Telegram gateway if enabled
            if config.gateway.telegram.enabled {
                info!("Telegram gateway enabled — spawning bot");
                let telegram_config = config.clone();
                std::thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::error!("Failed to create Telegram runtime: {}", e);
                            return;
                        }
                    };
                    rt.block_on(async {
                        let (mut event_rx, reply_sender) =
                            crate::gateway::telegram::spawn_bot(telegram_config.clone());
                        let router = match crate::gateway::message_router::MessageRouter::new(
                            telegram_config,
                        ) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::error!("Failed to create message router: {}", e);
                                return;
                            }
                        };

                        while let Some(event) = event_rx.recv().await {
                            let mut unified =
                                match crate::gateway::unified_router::UnifiedRouter::new(
                                    router.config.clone(),
                                ) {
                                    Ok(u) => u,
                                    Err(e) => {
                                        tracing::error!("Failed to create unified router: {}", e);
                                        return;
                                    }
                                };
                            unified.handle_telegram_event(event, &reply_sender).await;
                        }
                    });
                });
            }

            #[cfg(feature = "slack")]
            // Spawn Slack gateway if enabled
            if config.gateway.slack.enabled {
                info!("Slack gateway enabled — spawning bot");
                let slack_config = config.clone();
                std::thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::error!("Failed to create Slack runtime: {}", e);
                            return;
                        }
                    };
                    rt.block_on(async {
                        let (mut event_rx, reply_sender) =
                            crate::gateway::slack::spawn_bot(slack_config.clone());
                        let router = match crate::gateway::message_router::MessageRouter::new(
                            slack_config,
                        ) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::error!("Failed to create message router: {}", e);
                                return;
                            }
                        };

                        while let Some(event) = event_rx.recv().await {
                            let mut unified =
                                match crate::gateway::unified_router::UnifiedRouter::new(
                                    router.config.clone(),
                                ) {
                                    Ok(u) => u,
                                    Err(e) => {
                                        tracing::error!("Failed to create unified router: {}", e);
                                        return;
                                    }
                                };
                            unified.handle_slack_event(event, &reply_sender).await;
                        }
                    });
                });
            }

            #[cfg(feature = "matrix")]
            // Spawn Matrix gateway if enabled
            if config.gateway.matrix.enabled {
                info!("Matrix gateway enabled — spawning bot");
                let matrix_config = config.clone();
                std::thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::error!("Failed to create Matrix runtime: {}", e);
                            return;
                        }
                    };
                    rt.block_on(async {
                        let (mut event_rx, reply_sender) =
                            crate::gateway::matrix::spawn_bot(matrix_config.clone());
                        let router =
                            match crate::gateway::message_router::MessageRouter::new(matrix_config)
                            {
                                Ok(r) => r,
                                Err(e) => {
                                    tracing::error!("Failed to create message router: {}", e);
                                    return;
                                }
                            };

                        while let Some(event) = event_rx.recv().await {
                            let mut unified =
                                match crate::gateway::unified_router::UnifiedRouter::new(
                                    router.config.clone(),
                                ) {
                                    Ok(u) => u,
                                    Err(e) => {
                                        tracing::error!("Failed to create unified router: {}", e);
                                        return;
                                    }
                                };
                            unified.handle_matrix_event(event, &reply_sender).await;
                        }
                    });
                });
            }

            tui::run(config).await?;
        }
        Some(Commands::Setup) => {
            println!("🦈 OpenShark Setup");
            println!("Run `openshark` to start the TUI.");
            config::setup::run().await?;
        }
        Some(Commands::Config) => {
            println!("🦈 OpenShark Config");
            let config_path = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("openshark")
                .join("config.toml");
            println!("Config file: {}", config_path.display());
            if config_path.exists() {
                match std::fs::read_to_string(&config_path) {
                    Ok(contents) => println!("{}", contents),
                    Err(e) => println!("❌ Error reading config: {}", e),
                }
            } else {
                println!("No config file found. Run `openshark setup` to create one.");
            }
        }
        Some(Commands::Stats) => {
            println!("🦈 OpenShark Stats");
            println!();

            let memory = match memory::MemoryStore::new(&config.memory_db_path) {
                Ok(m) => m,
                Err(e) => {
                    println!("❌ Error opening memory store: {}", e);
                    return Ok(());
                }
            };

            match memory.get_stats_summary() {
                Ok(stats) => {
                    println!("📊 Session Overview");
                    println!("{}", "─".repeat(50));
                    println!("  Total Sessions:    {}", stats.total_sessions);
                    println!("  Total Messages:    {}", stats.total_messages);
                    println!("  Total Tool Calls:  {}", stats.total_tool_calls);
                    println!(
                        "  Successful Tools:  {} ({:.1}%)",
                        stats.successful_tool_calls, stats.tool_success_rate
                    );
                    println!("  Total Tokens:      {}", stats.total_tokens);
                    println!("  Unique Models:     {}", stats.unique_models);
                    if let Some(first) = stats.first_session {
                        println!("  First Session:     {}", first.format("%Y-%m-%d %H:%M"));
                    }
                    if let Some(latest) = stats.latest_session {
                        println!("  Latest Session:    {}", latest.format("%Y-%m-%d %H:%M"));
                    }
                    println!();
                }
                Err(e) => println!("❌ Error loading stats: {}", e),
            }

            match memory.get_model_usage_stats() {
                Ok(models) if !models.is_empty() => {
                    println!("🤖 Model Usage");
                    println!("{}", "─".repeat(70));
                    println!(
                        "  {:<20} | {:>8} | {:>8} | {:>10} | {:>6}",
                        "Model", "Sessions", "Messages", "Tokens", "Tools%"
                    );
                    println!("{}", "─".repeat(70));
                    for m in models {
                        println!(
                            "  {:<20} | {:>8} | {:>8} | {:>10} | {:>5.1}%",
                            &m.model[..m.model.len().min(20)],
                            m.session_count,
                            m.message_count,
                            m.total_tokens,
                            m.tool_success_rate
                        );
                    }
                    println!();
                }
                _ => {}
            }

            match memory.get_tool_usage_stats() {
                Ok(tools) if !tools.is_empty() => {
                    println!("🔧 Tool Usage");
                    println!("{}", "─".repeat(50));
                    println!(
                        "  {:<15} | {:>8} | {:>8} | {:>6}",
                        "Tool", "Calls", "Success", "Rate%"
                    );
                    println!("{}", "─".repeat(50));
                    for t in tools {
                        println!(
                            "  {:<15} | {:>8} | {:>8} | {:>5.1}%",
                            t.tool_name, t.total_calls, t.successful_calls, t.success_rate
                        );
                    }
                    println!();
                }
                _ => {}
            }

            match memory.get_daily_activity(30) {
                Ok(days) if !days.is_empty() => {
                    println!("📅 Recent Activity (last {} days)", days.len());
                    println!("{}", "─".repeat(40));
                    println!("  {:<12} | {:>8} | {:>8}", "Date", "Sessions", "Models");
                    println!("{}", "─".repeat(40));
                    for day in days.iter().take(7) {
                        println!(
                            "  {:<12} | {:>8} | {:>8}",
                            day.day, day.session_count, day.model_count
                        );
                    }
                    println!();
                }
                _ => {}
            }

            match router::get_router_stats(&config).await {
                Ok(router_stats) => {
                    println!("🎯 Routing Stats");
                    println!("{}", "─".repeat(50));
                    println!("  Total Routes:      {}", router_stats.total_routes);
                    println!("  Avg Success Rate:  {:.1}%", router_stats.avg_success_rate);
                    println!(
                        "  Top Model:         {} ({} uses)",
                        router_stats.top_model, router_stats.top_model_usage
                    );
                    println!();
                }
                Err(e) => println!("❌ Error loading router stats: {}", e),
            }

            let cache = cache::ResponseCache::new();
            if let Ok(cache) = cache {
                let cache_stats = cache.get_stats();
                let cache_size = cache.len();
                let total_requests = cache_stats.hits + cache_stats.misses;
                let hit_rate = if total_requests > 0 {
                    (cache_stats.hits as f64 / total_requests as f64) * 100.0
                } else {
                    0.0
                };

                println!("💾 Cache Stats");
                println!("{}", "─".repeat(50));
                println!("  Entries:           {}", cache_size);
                println!("  Hits:              {}", cache_stats.hits);
                println!("  Misses:            {}", cache_stats.misses);
                println!("  Sets:              {}", cache_stats.sets);
                println!("  Hit Rate:          {:.1}%", hit_rate);
                println!();
            }

            match memory.get_performance_summary() {
                Ok(perf) if perf.total_requests > 0 => {
                    println!("⚡ Performance Metrics");
                    println!("{}", "─".repeat(50));
                    println!("  Avg First Token:   {}ms", perf.avg_first_token_ms);
                    println!("  Avg Total Latency: {}ms", perf.avg_total_latency_ms);
                    println!("  Avg Tool Exec:     {}ms", perf.avg_tool_execution_ms);
                    println!("  P95 First Token:   {}ms", perf.p95_first_token_ms);
                    println!("  P95 Tool Exec:     {}ms", perf.p95_tool_execution_ms);
                    println!("  Total Requests:    {}", perf.total_requests);
                    println!("  Total Tool Calls:  {}", perf.total_tools);
                    println!();
                }
                _ => {
                    println!("⚡ Performance Metrics");
                    println!("{}", "─".repeat(50));
                    println!("  No performance data collected yet.");
                    println!("  Start a TUI session to begin tracking.");
                    println!();
                }
            }

            println!("💰 Cost Tracking");
            println!("{}", "─".repeat(50));
            let mut total_estimated_cost = 0.0;
            for provider in config.providers.values() {
                for model in &provider.models {
                    let cost_per_1k = model.cost_per_1k_input + model.cost_per_1k_output;
                    if cost_per_1k > 0.0 {
                        println!("  {:<20} | ${:>8.4}/1K tokens", model.name, cost_per_1k);
                        total_estimated_cost += cost_per_1k;
                    } else {
                        println!("  {:<20} | Free", model.name);
                    }
                }
            }
            if total_estimated_cost > 0.0 {
                println!("  {:<20} | ${:>8.4} total/1K", "", total_estimated_cost);
            }
            println!("  Cost Limit:        ${:.2}", config.cost_limit_usd);
        }
        Some(Commands::Memory {
            query,
            recent,
            semantic,
            limit,
        }) => {
            if query.is_empty() && !recent {
                println!("🦈 Memory Query");
                println!("Usage: openshark memory <query>");
                println!("       openshark memory --recent [--limit 5]");
                println!("       openshark memory <query> --semantic [--limit 10]");
            } else if recent {
                let memory = memory::MemoryStore::new(&config.memory_db_path)?;
                match memory.get_recent_sessions(limit) {
                    Ok(sessions) => {
                        println!("🦈 Recent Sessions (last {}):", limit);
                        for s in sessions {
                            println!(
                                "  {} | {} | {} | {}",
                                &s.id[..8.min(s.id.len())],
                                s.started_at.format("%Y-%m-%d %H:%M"),
                                s.model,
                                s.task_type
                            );
                        }
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            } else if semantic {
                let memory = memory::MemoryStore::new(&config.memory_db_path)?;
                match memory.semantic_search(&query, limit) {
                    Ok(results) => {
                        println!(
                            "🦈 Semantic Search: '{}' ({} results)",
                            query,
                            results.len()
                        );
                        for (msg, score) in results {
                            let preview = &msg.content[..msg.content.len().min(100)];
                            println!(
                                "  [{:.3}] [{}] {}: {}",
                                score,
                                msg.created_at.format("%Y-%m-%d %H:%M"),
                                msg.role,
                                preview
                            );
                        }
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            } else {
                let memory = memory::MemoryStore::new(&config.memory_db_path)?;
                match memory.search_messages(&query, limit) {
                    Ok(messages) => {
                        println!("🦈 Memory Search: '{}' ({} results)", query, messages.len());
                        for msg in messages {
                            let preview = &msg.content[..msg.content.len().min(100)];
                            println!(
                                "  [{}] {}: {}",
                                msg.created_at.format("%Y-%m-%d %H:%M"),
                                msg.role,
                                preview
                            );
                        }
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            }
        }
        Some(Commands::Route) => {
            router::show_decisions(&config).await?;
        }
        Some(Commands::Learn) => {
            self_improve::trigger_analysis(&config).await?;
        }
        Some(Commands::Agent { task }) => {
            if task.is_empty() {
                println!("🦈 Agent Mode");
                println!("Usage: openshark agent <task>");
                println!("       openshark agent 'fix the bug in src/main.rs'");
            } else {
                let agent_config = agent::AgentConfig::default();
                let agent = agent::Agent::new(agent_config, &config)?;
                match agent.run_task(&task).await {
                    Ok(result) => {
                        println!(
                            "\n🦈 Agent Result: {}",
                            if result.success {
                                "✅ Success"
                            } else {
                                "⚠️ Partial"
                            }
                        );
                        println!("Message: {}", result.message);
                        println!("Total iterations: {}", result.total_iterations);
                        for (i, step) in result.step_results.iter().enumerate() {
                            println!(
                                "  Step {}: {} {} → verified={} ({} iter)",
                                i + 1,
                                step.step.tool_name,
                                step.step.args,
                                step.verified,
                                step.iterations
                            );
                        }
                    }
                    Err(e) => println!("❌ Agent error: {}", e),
                }
            }
        }
        Some(Commands::Test { cmd, path }) => {
            let test_tool = tools::test_runner::TestTool;
            match test_tool.execute(&format!("{} {}", cmd, path)) {
                Ok(result) => println!("{}", result),
                Err(e) => println!("❌ Error: {}", e),
            }
        }
        Some(Commands::Models) => {
            println!("🦈 Available Models");
            println!();
            for (provider_name, provider) in &config.providers {
                println!("Provider: {} ({})", provider_name, provider.base_url);
                for model in &provider.models {
                    let cost = model.cost_per_1k_input + model.cost_per_1k_output;
                    let cost_str = if cost > 0.0 {
                        format!("${:.4}/1K", cost)
                    } else {
                        "Free".to_string()
                    };
                    let default_marker = if model.name == config.default_model {
                        " [default]"
                    } else {
                        ""
                    };
                    println!(
                        "  • {} | ctx={} | {} | capabilities: {}{}",
                        model.name,
                        model.context_length,
                        cost_str,
                        model.capabilities.join(", "),
                        default_marker
                    );
                }
                println!();
            }
        }
        Some(Commands::Chat { message, model, file }) => {
            if message.is_empty() && file.is_none() {
                println!("🦈 One-shot Chat");
                println!("Usage: openshark chat 'your message here'");
                println!("       openshark chat 'hello' --model kimi-k2.6");
                println!("       openshark chat 'review this' --file src/main.rs");
            } else {
                let model_name = model.as_deref().unwrap_or(&config.default_model);
                let (provider_name, provider_config) = config
                    .find_provider_for_model(model_name)
                    .unwrap_or_else(|| {
                        println!("⚠️  Model '{}' not found, using default", model_name);
                        let (n, p) = config.providers.iter().next().unwrap();
                        (n.clone(), p.clone())
                    });

                println!("🦈 Chat with {} (via {})", model_name, provider_name);
                println!();

                let provider = providers::Provider::new(
                    provider_name.clone(),
                    provider_config.base_url.clone(),
                    provider_config.api_key.clone(),
                    provider_config.kind.clone(),
                    provider_config.headers.clone(),
                );

                // Build a proper system prompt with tool instructions
                let tool_descriptions = tools::get_tools()
                    .iter()
                    .map(|t| format!("- {}: {}", t.name(), t.description()))
                    .collect::<Vec<_>>()
                    .join("\n");

                let system_prompt = format!(
                    "You are an AI coding assistant with TOOL ACCESS. You MUST use tools when asked to perform actions. \
                     You are NOT a generic chatbot — you are an agent with real tool capabilities. \
                     \n\nAVAILABLE TOOLS:\n{}\n\nTOOL USAGE RULES:\n\
                     1. When asked to do something requiring a tool, output ONLY: TOOL:<tool_name> <args>\n\
                     2. Do NOT say 'I don't have access' or 'I cannot execute commands'. You DO have access.\n\
                     3. Do NOT preamble. Just output the TOOL: line.\n\
                     4. Low and Medium risk tools execute automatically.\n\
                     5. High risk tools require user approval.\n\
                     6. If the user says 'test', run the test tool immediately.\n\
                     7. For one-line tasks, just do it. No manifesto.",
                    tool_descriptions
                );

                let mut user_content = message.clone();

                // Attach file content if provided
                if let Some(ref path) = file {
                    match std::fs::read_to_string(path) {
                        Ok(content) => {
                            println!("📎 Attached file: {} ({} bytes)", path, content.len());
                            user_content.push_str(&format!("\n\n--- File: {} ---\n```\n{}\n```", path, content));
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to read file {}: {}", path, e);
                            std::process::exit(1);
                        }
                    }
                }

                let messages = vec![
                    providers::Message {
                        role: "system".to_string(),
                        content: system_prompt,
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                    },
                    providers::Message {
                        role: "user".to_string(),
                        content: user_content,
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                        reasoning_content: None,
                    },
                ];

                let mut request =
                    providers::ChatRequest::new(model_name.to_string(), messages, true);
                // Attach tools so the model knows it can call them
                request.tools = Some(tools::get_openai_tool_definitions());

                match provider.chat_stream(request).await {
                    Ok((chunks, metrics)) => {
                        let mut full_response = String::new();
                        for chunk in chunks {
                            print!("{}", chunk);
                            full_response.push_str(&chunk);
                        }
                        println!();
                        println!();

                        // Parse and execute any embedded TOOL: lines from the response
                        let embedded_tools = parse_embedded_tools_cli(&full_response);
                        if !embedded_tools.is_empty() {
                            for (tool_name, args) in embedded_tools {
                                println!("🔧 Executing: {} {}", tool_name, args);
                                match tools::find_tool(&tool_name) {
                                    Some(tool) => match tool.execute(&args) {
                                        Ok(result) => {
                                            println!("✅ Result:\n{}", result);
                                        }
                                        Err(e) => {
                                            println!("❌ Error: {}", e);
                                        }
                                    },
                                    None => {
                                        println!("❌ Unknown tool: {}", tool_name);
                                    }
                                }
                            }
                        }

                        println!();
                        println!(
                            "⚡ First token: {}ms | Total: {}ms | Tokens: {}",
                            metrics.first_token_latency_ms,
                            metrics.total_latency_ms,
                            metrics.tokens_generated
                        );
                    }
                    Err(e) => println!("❌ Error: {}", e),
                }
            }
        }

        Some(Commands::Security { cmd, arg }) => {
            match cmd.as_str() {
                "status" => {
                    let sec_config = security::SecurityConfig::load()?;
                    println!("🔒 OpenShark Security Status");
                    println!("{}", "─".repeat(60));
                    println!("  Version:           {}", sec_config.version);
                    println!("  Working Dir:       {:?}", sec_config.working_directory);
                    println!(
                        "  Allow Escape:      {}",
                        sec_config.allow_escape_working_dir
                    );
                    println!(
                        "  PII Redaction:     {}",
                        if sec_config.pii_redaction_enabled {
                            "✅"
                        } else {
                            "❌"
                        }
                    );
                    println!(
                        "  Injection Detect:  {}",
                        if sec_config.prompt_injection_detection_enabled {
                            "✅"
                        } else {
                            "❌"
                        }
                    );
                    println!(
                        "  Auto-approve:      {:?}",
                        sec_config.auto_approve_risk_level
                    );
                    println!(
                        "  Max Output:        {} bytes",
                        sec_config.max_model_output_bytes
                    );
                    println!();
                    println!(
                        "  Sudo:              {}",
                        if sec_config.sudo.enabled {
                            "✅ Enabled"
                        } else {
                            "❌ Disabled"
                        }
                    );
                    println!(
                        "  Sudo Persist:      {}",
                        if sec_config.sudo.persist_password {
                            "✅"
                        } else {
                            "❌"
                        }
                    );
                    println!();
                    println!(
                        "  Zero-Trust:        {}",
                        if sec_config.identity.zero_trust_enabled {
                            "✅"
                        } else {
                            "❌"
                        }
                    );
                    println!(
                        "  Max Sessions:      {}",
                        sec_config.identity.max_concurrent_sessions
                    );
                    println!(
                        "  Credential TTL:    {}s",
                        sec_config.identity.credential_ttl_secs
                    );
                    println!();
                    println!("  Tool Permissions:");
                    for (tool, perm) in &sec_config.tool_permissions {
                        let icon = match perm {
                            security::PermissionLevel::Allow => "✅",
                            security::PermissionLevel::Ask => "⚠️ ",
                            security::PermissionLevel::Deny => "❌",
                        };
                        println!("    {} {}: {:?}", icon, tool, perm);
                    }
                }
                "audit" => {
                    let sec_config = security::SecurityConfig::load()?;
                    let engine = security::SecurityEngine::new(sec_config)?;
                    let limit = arg.parse::<usize>().unwrap_or(10);
                    let entries = engine.get_audit_log(limit);
                    println!("🔒 Security Audit Log (last {} entries)", entries.len());
                    println!("{}", "─".repeat(80));
                    for entry in entries {
                        println!(
                            "  [{}] {} {} {:?} approved={} {}",
                            entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
                            entry.tool,
                            entry.action,
                            entry.risk_level,
                            entry.approved,
                            entry.reason
                        );
                    }
                }
                "test" => {
                    let sec_config = security::SecurityConfig::load()?;
                    let engine = security::SecurityEngine::new(sec_config)?;
                    let test_input = if arg.is_empty() { "hello world" } else { &arg };
                    println!("🔒 Security Test");
                    println!("{}", "─".repeat(60));
                    println!("  Input: '{}'", test_input);

                    // Test PII detection
                    let pii_findings = engine.pii_detector.scan(test_input);
                    println!("  PII findings: {}", pii_findings.len());
                    for finding in &pii_findings {
                        println!("    - [{}] {}", finding.category, finding.snippet);
                    }

                    // Test prompt injection
                    if let Some(injection) = engine.check_prompt_injection(test_input) {
                        println!("  ⚠️  Injection detected: {}", injection);
                    } else {
                        println!("  ✅ No injection detected");
                    }

                    // Test tool call check
                    let decision = engine.check_tool_call("terminal", test_input);
                    println!("  Tool check: {:?}", decision);
                }
                _ => {
                    println!("🔒 Security Commands");
                    println!("  openshark security status       - Show security configuration");
                    println!("  openshark security audit [n]    - Show audit log (default 10)");
                    println!("  openshark security test [input] - Test security detection");
                }
            }
        }
        Some(Commands::Mcp { cmd }) => match cmd.as_str() {
            "status" => {
                println!("🔌 MCP Status");
                println!("{}", "─".repeat(60));

                if !config.gateway.mcp.enabled {
                    println!("  MCP is disabled in config.");
                    println!("  Set [gateway.mcp] enabled = true to enable.");
                } else if config.gateway.mcp.servers.is_empty() {
                    println!("  MCP enabled but no servers configured.");
                    println!("  Add servers under [[gateway.mcp.servers]] in config.");
                } else {
                    println!("  Configured servers: {}", config.gateway.mcp.servers.len());
                    for server in &config.gateway.mcp.servers {
                        let transport_type = match &server.transport {
                            crate::gateway::McpTransport::Stdio { command, .. } => {
                                format!("stdio: {}", command)
                            }
                            crate::gateway::McpTransport::Sse { url, .. } => {
                                format!("sse: {}", url)
                            }
                        };
                        println!("  • {} ({})", server.name, transport_type);
                    }
                    println!();
                    println!("  Run `openshark` (TUI mode) to connect to MCP servers.");
                }
            }
            "tools" => {
                println!("🔌 MCP Tools");
                println!("{}", "─".repeat(60));
                println!("  MCP tools are discovered dynamically at runtime.");
                println!("  Start the TUI to connect to servers and discover tools.");
            }
            _ => {
                println!("🔌 MCP Commands");
                println!("  openshark mcp status - Show MCP configuration");
                println!("  openshark mcp tools  - Show tool discovery info");
            }
        },
        Some(Commands::Swarm { cmd, prompt }) => match cmd.as_str() {
            "init" => {
                if prompt.is_empty() {
                    println!("🐝 Swarm Mode");
                    println!("Usage: openshark swarm init 'your seed prompt here'");
                    println!();
                    println!("Example:");
                    println!("  openshark swarm init 'Build a REST API with auth'");
                } else {
                    println!("🐝 Initializing swarm...");
                    let swarm_config = config.swarm.clone();
                    let engine = swarm::SwarmEngine::new(swarm_config);
                    match engine.init(&prompt, &config).await {
                        Ok(()) => {
                            println!(
                                "✅ Swarm initialized with {} agents",
                                engine.agent_snapshot().await.len()
                            );
                            println!();
                            for agent in engine.agent_snapshot().await {
                                println!(
                                    "  🐝 {} ({}) - {}",
                                    agent.name, agent.role.name, agent.status
                                );
                            }
                            println!();
                            println!("  Run `openshark swarm start` to begin the autonomous loop.");
                        }
                        Err(e) => println!("❌ Failed to initialize swarm: {}", e),
                    }
                }
            }
            "start" => {
                println!("🐝 Starting swarm...");
                let swarm_config = config.swarm.clone();
                let engine = swarm::SwarmEngine::new(swarm_config);
                match engine.start().await {
                    Ok(()) => {
                        println!("✅ Swarm loop started");
                        println!("  Run `openshark swarm status` to check progress.");
                    }
                    Err(e) => println!("❌ Failed to start swarm: {}", e),
                }
            }
            "stop" => {
                println!("🐝 Stopping swarm...");
                let swarm_config = config.swarm.clone();
                let engine = swarm::SwarmEngine::new(swarm_config);
                match engine.stop().await {
                    Ok(()) => println!("✅ Swarm stopped"),
                    Err(e) => println!("❌ Failed to stop swarm: {}", e),
                }
            }
            "status" => {
                let swarm_config = config.swarm.clone();
                let engine = swarm::SwarmEngine::new(swarm_config);
                let status = engine.status().await;
                println!("{}", status);
            }
            _ => {
                println!("🐝 Swarm Commands");
                println!("  openshark swarm init 'prompt'  - Initialize swarm with seed prompt");
                println!("  openshark swarm start           - Start autonomous loop");
                println!("  openshark swarm stop            - Stop swarm");
                println!("  openshark swarm status          - Show swarm status");
                println!();
                println!("  Roles: {:?}", config.swarm.roles);
            }
        },
        Some(Commands::Tools { cmd }) => match cmd.as_str() {
            "list" | "" => {
                println!(
                    "🦈 OpenShark Tools — {} total\n",
                    crate::tools::get_tools().len()
                );

                println!("🔧 Native Tools:");
                for tool in crate::tools::get_native_tools() {
                    println!("  {} — {}", tool.name(), tool.description());
                }

                println!("\n⚡ Capability Tools:");
                for tool in crate::tools::get_capability_tools() {
                    println!("  {} — {}", tool.name(), tool.description());
                }

                println!("\n💡 Usage:");
                println!("  In agent mode, the model can invoke any tool.");
                println!(
                    "  In TUI, use TOOL:<tool_name> <args> or TOOL.<tool_name> <args> to execute manually."
                );
            }
            _ => {
                println!("🦈 Tools Commands");
                println!("  openshark tools list  - Show all available tools");
            }
        },
        Some(Commands::Doctor { fix, component: _ }) => {
            let report = crate::doctor::run_checks(fix).await?;
            report.print();
        }
        Some(Commands::Plugins { cmd, name }) => match cmd.as_str() {
            "list" | "" => {
                crate::plugins::list_plugins_cli();
            }
            "create" if !name.is_empty() => {
                let registry = crate::plugins::PluginRegistry::new();
                match registry.create_scaffold(&name) {
                    Ok(path) => println!("✅ Plugin scaffold created at {}", path.display()),
                    Err(e) => println!("❌ Failed to create plugin: {}", e),
                }
            }
            "enable" if !name.is_empty() => {
                let mut registry = crate::plugins::PluginRegistry::new();
                match registry.enable(&name) {
                    Ok(()) => println!("✅ Plugin '{}' enabled", name),
                    Err(e) => println!("❌ {}", e),
                }
            }
            "disable" if !name.is_empty() => {
                let mut registry = crate::plugins::PluginRegistry::new();
                match registry.disable(&name) {
                    Ok(()) => println!("✅ Plugin '{}' disabled", name),
                    Err(e) => println!("❌ {}", e),
                }
            }
            _ => {
                println!("🦈 Plugin Commands");
                println!("  openshark plugins list           - List all plugins");
                println!("  openshark plugins create <name>  - Create plugin scaffold");
                println!("  openshark plugins enable <name>  - Enable a plugin");
                println!("  openshark plugins disable <name> - Disable a plugin");
            }
        },
        Some(Commands::Delegate { agent, task }) => {
            if agent.is_empty() || task.is_empty() {
                println!("🦈 Delegate — Route tasks to external agents");
                println!();
                println!("Usage: openshark delegate <agent> <task>");
                println!("       openshark delegate claw 'refactor auth module'");
                println!("       openshark delegate opencode 'fix bug #42'");
                println!("       openshark delegate claude 'write tests for src/lib.rs'");
                println!();
                println!("Available agents:");
                for a in integrations::registry::available() {
                    println!("  • {}", a);
                }
                if integrations::registry::available().is_empty() {
                    println!("  (none detected — install claw, opencode, or claude-code)");
                }
            } else {
                match agent.parse::<integrations::registry::Agent>() {
                    Ok(a) => {
                        println!("🦈 Delegating to {}: {}", a, task);
                        match integrations::registry::delegate(a, &task, 300) {
                            Ok(result) => println!("{}", result),
                            Err(e) => println!("❌ Delegation failed: {}", e),
                        }
                    }
                    Err(e) => println!("❌ Unknown agent: {}", e),
                }
            }
        }
        Some(Commands::Hermes { cmd }) => match cmd.as_str() {
            "status" => {
                println!("🦈 Hermes Bridge");
                println!("{}", "─".repeat(50));
                let detected = integrations::hermes::detect();
                println!("  Hermes detected: {}", if detected { "✅" } else { "❌" });
                if detected {
                    println!("  Run `openshark hermes sync` to pull memories.");
                    println!("  Run `openshark hermes push` to push skills.");
                } else {
                    println!("  Install Hermes Agent to enable bridge.");
                }
            }
            "sync" => {
                match integrations::hermes::sync_pull("~/.hermes") {
                    Ok(result) => println!("✅ {}", result),
                    Err(e) => println!("❌ Sync failed: {}", e),
                }
            }
            "push" => {
                match integrations::hermes::sync_push("~/.hermes") {
                    Ok(result) => println!("✅ {}", result),
                    Err(e) => println!("❌ Push failed: {}", e),
                }
            }
            _ => {
                println!("🦈 Hermes Commands");
                println!("  openshark hermes status - Show bridge status");
                println!("  openshark hermes sync   - Pull memories from Hermes");
                println!("  openshark hermes push   - Push skills to Hermes");
            }
        },
        Some(Commands::Headless { task, yolo, json, timeout, max_turns, model, output }) => {
            println!("🦈 OpenShark Headless Mode");
            let mut cfg = config.clone();
            if let Some(ref m) = model {
                cfg.default_model = m.clone();
            }
            let headless_config = crate::headless::HeadlessConfig {
                task,
                yolo,
                json,
                max_turns,
                timeout_secs: timeout,
                model,
                output_file: output,
            };
            let provider = match crate::swarm::agent_runner::build_agent_provider(&cfg) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("❌ Failed to initialize provider: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = crate::headless::run_headless(headless_config, provider, cfg.default_model, None).await {
                eprintln!("❌ Headless run failed: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::RepoMap { path }) => {
            println!("🦈 Repo Map");
            match crate::repo_map::build_repo_map(&path) {
                Ok(map) => {
                    println!("{}", crate::repo_map::format_repo_map(&map));
                }
                Err(e) => {
                    eprintln!("❌ Failed to build repo map: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Lint { path }) => {
            println!("🦈 Lint");
            match crate::linting::detect_linter(&path) {
                Some(linter) => {
                    println!("Detected linter: {}", linter);
                    match crate::linting::run_linter(&path).await {
                        Ok(results) => {
                            for result in &results {
                                println!("[{}] {}:{} — {}", result.severity, result.file, result.line, result.message);
                            }
                            if results.iter().any(|r| r.severity == crate::linting::Severity::Error) {
                                std::process::exit(1);
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Linter failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("❌ No supported linter detected in {}", path);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::McpServer) => {
            println!("🦈 OpenShark MCP Server");
            let server = crate::mcp_server::McpServer::new();
            if let Err(e) = server.run_stdio().await {
                eprintln!("❌ MCP server error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Diff) => {
            println!("🦈 Diff — AI-made changes");
            let git_tool = crate::tools::GitTool;
            match git_tool.execute("diff") {
                Ok(diff) => {
                    if diff.trim().is_empty() {
                        println!("📭 No unstaged changes.");
                    } else {
                        println!("{}", diff);
                    }
                }
                Err(e) => {
                    eprintln!("❌ git diff failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Watch { path, cmd, debounce }) => {
            let command = match cmd.as_str() {
                "test" => crate::watch::WatchCommand::Test,
                "lint" => crate::watch::WatchCommand::Lint,
                "build" => crate::watch::WatchCommand::Build,
                _ => crate::watch::WatchCommand::Custom(cmd),
            };
            let config = crate::watch::WatchConfig {
                path,
                debounce_ms: debounce,
                command,
            };
            if let Err(e) = crate::watch::run_watch(config) {
                eprintln!("❌ Watch mode error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Profile { name }) => {
            if name.is_empty() {
                println!("🦈 Config Profiles");
                println!("Usage: openshark profile <name>");
                println!();
                println!("Profiles are stored in ~/.config/openshark/profiles/");
                println!("Each profile is a separate config.json with its own model, provider, and settings.");
            } else {
                let profile_dir = dirs::config_dir()
                    .map(|d| d.join("openshark").join("profiles"))
                    .unwrap_or_else(|| PathBuf::from(".openshark/profiles"));
                let profile_path = profile_dir.join(format!("{}.json", name));
                if profile_path.exists() {
                    println!("✅ Profile '{}' found at {}", name, profile_path.display());
                    println!("   To use: set OPENSHARK_PROFILE={} or use --profile flag (coming soon)", name);
                } else {
                    println!("📭 Profile '{}' not found.", name);
                    println!("   Create one by copying your config:");
                    println!("   cp ~/.config/openshark/config.json {}", profile_path.display());
                }
            }
        }
        Some(Commands::Export { name }) => {
            println!("🦈 Export session to markdown");
            let memory = match memory::MemoryStore::new(&config.memory_db_path) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("❌ Failed to open memory store: {}", e);
                    std::process::exit(1);
                }
            };
            match memory.get_recent_sessions(1) {
                Ok(sessions) => {
                    if let Some(session) = sessions.first() {
                        let filename = format!("{}_{}.md", name, session.id);
                        let mut md = format!("# Session Export: {}\n\n", session.id);
                        md.push_str(&format!("- **Model:** {}\n", session.model));
                        md.push_str(&format!("- **Started:** {}\n", session.started_at));
                        md.push_str("---\n\n");
                        match memory.get_session_messages(&session.id) {
                            Ok(messages) => {
                                for msg in messages {
                                    md.push_str(&format!("## {}\n\n{}", msg.role, msg.content));
                                    md.push_str("\n\n---\n\n");
                                }
                            }
                            Err(e) => eprintln!("⚠️ Failed to load messages: {}", e),
                        }
                        match std::fs::write(&filename, md) {
                            Ok(_) => println!("✅ Exported to {}", filename),
                            Err(e) => {
                                eprintln!("❌ Failed to write {}: {}", filename, e);
                                std::process::exit(1);
                            }
                        }
                    } else {
                        println!("📭 No sessions found to export.");
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to get sessions: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
