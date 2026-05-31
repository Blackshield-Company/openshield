use clap::{Parser, Subcommand};
use tracing::info;

/// The current version of OpenShark.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod agent;
mod cache;
mod capabilities;
mod config;
mod evolution;
mod gateway;
mod image_utils;
mod lsp;
mod mcp;
mod memory;
mod providers;
mod router;
mod security;
mod self_improve;
mod skills;
mod swarm;
mod tools;
mod tui;

use config::Config;
use crate::tools::Tool;

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Config::load_or_default()?;

    match cli.command {
        Some(Commands::Tui) | None => {
            info!("Starting OpenShark TUI");

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
                        let mut event_rx = crate::gateway::discord::spawn_bot(discord_config.clone());
                        let mut router = match crate::gateway::message_router::MessageRouter::new(discord_config) {
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
                        let (mut event_rx, reply_sender) = crate::gateway::telegram::spawn_bot(telegram_config.clone());
                        let router = match crate::gateway::message_router::MessageRouter::new(telegram_config) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::error!("Failed to create message router: {}", e);
                                return;
                            }
                        };

                        while let Some(event) = event_rx.recv().await {
                            let mut unified = match crate::gateway::unified_router::UnifiedRouter::new(router.config.clone()) {
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
                        let (mut event_rx, reply_sender) = crate::gateway::slack::spawn_bot(slack_config.clone());
                        let router = match crate::gateway::message_router::MessageRouter::new(slack_config) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::error!("Failed to create message router: {}", e);
                                return;
                            }
                        };

                        while let Some(event) = event_rx.recv().await {
                            let mut unified = match crate::gateway::unified_router::UnifiedRouter::new(router.config.clone()) {
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
                        let (mut event_rx, reply_sender) = crate::gateway::matrix::spawn_bot(matrix_config.clone());
                        let router = match crate::gateway::message_router::MessageRouter::new(matrix_config) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::error!("Failed to create message router: {}", e);
                                return;
                            }
                        };

                        while let Some(event) = event_rx.recv().await {
                            let mut unified = match crate::gateway::unified_router::UnifiedRouter::new(router.config.clone()) {
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
                    println!("  Successful Tools:  {} ({:.1}%)", stats.successful_tool_calls, stats.tool_success_rate);
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
                    println!("  {:<20} | {:>8} | {:>8} | {:>10} | {:>6}",
                             "Model", "Sessions", "Messages", "Tokens", "Tools%");
                    println!("{}", "─".repeat(70));
                    for m in models {
                        println!("  {:<20} | {:>8} | {:>8} | {:>10} | {:>5.1}%",
                                 &m.model[..m.model.len().min(20)],
                                 m.session_count,
                                 m.message_count,
                                 m.total_tokens,
                                 m.tool_success_rate);
                    }
                    println!();
                }
                _ => {}
            }

            match memory.get_tool_usage_stats() {
                Ok(tools) if !tools.is_empty() => {
                    println!("🔧 Tool Usage");
                    println!("{}", "─".repeat(50));
                    println!("  {:<15} | {:>8} | {:>8} | {:>6}",
                             "Tool", "Calls", "Success", "Rate%");
                    println!("{}", "─".repeat(50));
                    for t in tools {
                        println!("  {:<15} | {:>8} | {:>8} | {:>5.1}%",
                                 t.tool_name,
                                 t.total_calls,
                                 t.successful_calls,
                                 t.success_rate);
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
                        println!("  {:<12} | {:>8} | {:>8}",
                                 day.day,
                                 day.session_count,
                                 day.model_count);
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
                    println!("  Top Model:         {} ({} uses)", router_stats.top_model, router_stats.top_model_usage);
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
            for (_, provider) in &config.providers {
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
        Some(Commands::Memory { query, recent, semantic, limit }) => {
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
                            println!("  {} | {} | {} | {}",
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
                        println!("🦈 Semantic Search: '{}' ({} results)", query, results.len());
                        for (msg, score) in results {
                            let preview = &msg.content[..msg.content.len().min(100)];
                            println!("  [{:.3}] [{}] {}: {}",
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
                            println!("  [{}] {}: {}",
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
                        println!("\n🦈 Agent Result: {}", if result.success { "✅ Success" } else { "⚠️ Partial" });
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
        Some(Commands::Chat { message, model }) => {
            if message.is_empty() {
                println!("🦈 One-shot Chat");
                println!("Usage: openshark chat 'your message here'");
                println!("       openshark chat 'hello' --model kimi-k2.6");
            } else {
                let model_name = model.as_deref().unwrap_or(&config.default_model);
                let (provider_name, provider_config) = config.find_provider_for_model(model_name)
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

                let messages = vec![
                    providers::Message {
                        role: "system".to_string(),
                        content: "You are a helpful assistant.".to_string(),
                        images: None,
                    },
                    providers::Message {
                        role: "user".to_string(),
                        content: message.clone(),
                        images: None,
                    },
                ];

                let request = providers::ChatRequest::new(
                    model_name.to_string(),
                    messages,
                    true,
                );

                match provider.chat_stream(request).await {
                    Ok((chunks, metrics)) => {
                        let mut full_response = String::new();
                        for chunk in chunks {
                            print!("{}", chunk);
                            full_response.push_str(&chunk);
                        }
                        println!();
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
                    println!("  Allow Escape:      {}", sec_config.allow_escape_working_dir);
                    println!("  PII Redaction:     {}", if sec_config.pii_redaction_enabled { "✅" } else { "❌" });
                    println!("  Injection Detect:  {}", if sec_config.prompt_injection_detection_enabled { "✅" } else { "❌" });
                    println!("  Auto-approve:      {:?}", sec_config.auto_approve_risk_level);
                    println!("  Max Output:        {} bytes", sec_config.max_model_output_bytes);
                    println!();
                    println!("  Sudo:              {}", if sec_config.sudo.enabled { "✅ Enabled" } else { "❌ Disabled" });
                    println!("  Sudo Persist:      {}", if sec_config.sudo.persist_password { "✅" } else { "❌" });
                    println!();
                    println!("  Zero-Trust:        {}", if sec_config.identity.zero_trust_enabled { "✅" } else { "❌" });
                    println!("  Max Sessions:      {}", sec_config.identity.max_concurrent_sessions);
                    println!("  Credential TTL:    {}s", sec_config.identity.credential_ttl_secs);
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
                        println!("  [{}] {} {} {:?} approved={} {}",
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
        Some(Commands::Mcp { cmd }) => {
            match cmd.as_str() {
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
            }
        }
        Some(Commands::Swarm { cmd, prompt }) => {
            match cmd.as_str() {
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
                                println!("✅ Swarm initialized with {} agents", engine.agent_snapshot().await.len());
                                println!();
                                for agent in engine.agent_snapshot().await {
                                    println!("  🐝 {} ({}) - {}", agent.name, agent.role.name, agent.status);
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
            }
        }
        Some(Commands::Tools { cmd }) => {
            match cmd.as_str() {
                "list" | "" => {
                    println!("🦈 OpenShark Tools — {} total\n", crate::tools::get_tools().len());

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
                    println!("  In TUI, use TOOL:<tool_name> <args> to execute manually.");
                }
                _ => {
                    println!("🦈 Tools Commands");
                    println!("  openshark tools list  - Show all available tools");
                }
            }
        }
    }

    Ok(())
}
