use anyhow::Result;
use super::Config;

pub async fn run() -> Result<()> {
    println!("🦈 OpenShark Setup");
    println!("==================");
    println!();
    println!("OpenShark will create:");
    println!("  - Config: ~/.config/openshark/config.toml");
    println!("  - Memory: ~/.local/share/openshark/memory.db");
    println!();
    println!("Press Enter to continue or Ctrl+C to cancel...");
    
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    
    let config = Config::default();
    config.save()?;
    
    println!();
    println!("✅ Config saved!");
    println!("Run `openshark` to start.");
    
    Ok(())
}
