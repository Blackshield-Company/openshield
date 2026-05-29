pub mod setup;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: String,
    pub default_model: String,
    pub providers: HashMap<String, ProviderConfig>,
    pub memory_db_path: PathBuf,
    pub tools_enabled: Vec<String>,
    pub auto_route: bool,
    pub cost_limit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<ModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub context_length: usize,
    pub cost_per_1k_input: f64,
    pub cost_per_1k_output: f64,
    pub capabilities: Vec<String>,
}

impl Config {
    pub fn load_or_default() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("No config directory found")?
            .join("openshark");
        
        let config_path = config_dir.join("config.toml");
        
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            let config: Config = toml::from_str(&content)
                .context("Failed to parse config.toml")?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }
    
    pub fn save(&self) -> Result<()> {
        let config_dir = dirs::config_dir()
            .context("No config directory found")?
            .join("openshark");
        std::fs::create_dir_all(&config_dir)?;
        
        let config_path = config_dir.join("config.toml");
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        std::fs::write(&config_path, content)
            .with_context(|| format!("Failed to write {}", config_path.display()))?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut providers = HashMap::new();
        
        providers.insert("local".to_string(), ProviderConfig {
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            api_key: "local".to_string(),
            models: vec![
                ModelConfig {
                    name: "synthclaw-35b-128k".to_string(),
                    context_length: 128000,
                    cost_per_1k_input: 0.0,
                    cost_per_1k_output: 0.0,
                    capabilities: vec!["code".to_string(), "chat".to_string()],
                },
            ],
        });
        
        Config {
            version: "0.1.0".to_string(),
            default_model: "synthclaw-35b-128k".to_string(),
            providers,
            memory_db_path: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("openshark")
                .join("memory.db"),
            tools_enabled: vec!["fs".to_string(), "terminal".to_string()],
            auto_route: true,
            cost_limit_usd: 10.0,
        }
    }
}
