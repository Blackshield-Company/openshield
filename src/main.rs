use clap::{Parser, Subcommand};
use tracing::{info, warn};

mod config;
mod memory;
mod providers;
mod router;
mod self_improve;
mod tools;
mod tui;

use config::Config;

#[derive(Parser)]
#[command(name = "openshark")]
#[command(about = "🦈 The harness that learns. The agent that decides.")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure providers, models, and preferences
    Setup,
    /// View token usage and model performance
    Stats,
    /// Query persistent memory
    Memory {
        #[arg(default_value = "")]
        query: String,
        #[arg(short, long, default_value_t = false)]
        recent: bool,
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    /// Show current routing decisions
    Route,
    /// Trigger self-improvement analysis
    Learn,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Config::load_or_default()?;

    match cli.command {
        Some(Commands::Setup) => {
            println!("🦈 OpenShark Setup");
            println!("Run `openshark` to start the TUI.");
            config::setup::run().await?;
        }
        Some(Commands::Stats) => {
            println!("🦈 OpenShark Stats");
            println!("Stats not yet implemented.");
        }
        Some(Commands::Memory { query, recent, limit }) => {
            if query.is_empty() && !recent {
                println!("🦈 Memory Query");
                println!("Usage: openshark memory <query>");
                println!("       openshark memory --recent [--limit 5]");
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
            } else {
                let memory = memory::MemoryStore::new(&config.memory_db_path)?;
                match memory.search_messages(&query, 20) {
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
        None => {
            info!("Starting OpenShark TUI");
            tui::run(config).await?;
        }
    }

    Ok(())
}
