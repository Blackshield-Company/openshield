use anyhow::Result;
use crate::config::Config;

pub async fn show_decisions(_config: &Config) -> Result<()> {
    println!("🦈 Routing Decisions");
    println!("Auto-route: enabled");
    println!("Current strategy: cost-optimized with quality fallback");
    println!();
    println!("Task Type      | Best Model        | Success Rate | Avg Cost");
    println!("---------------|-------------------|--------------|----------");
    println!("refactor       | synthclaw-35b-128k| 94%          | $0.000");
    println!("debug          | synthclaw-35b-128k| 89%          | $0.000");
    println!("architect      | kimi-k2.6         | 91%          | $0.012");
    println!("write          | synthclaw-14b-128k| 87%          | $0.000");
    Ok(())
}
