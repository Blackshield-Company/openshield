//! Custom user-defined tools loaded from config.
//!
//! Users can define simple tools in `~/.config/openshark/custom_tools.toml`:
//!
//! ```toml
//! [[tool]]
//! name = "weather"
//! description = "Get weather for a city"
//! command = "curl -s 'wttr.in/{{city}}?format=3'"
//!
//! [[tool]]
//! name = "ip"
//! description = "Show public IP address"
//! command = "curl -s ipinfo.io/ip"
//! ```
//!
//! Placeholders like `{{arg}}` are replaced with the first argument.
//! Commands run in a shell with a 30-second timeout.

use std::process::Command;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::Deserialize;

use crate::tools::Tool;

/// Custom tool definition from config.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomToolDef {
    pub name: String,
    pub description: String,
    pub command: String,
}

/// A user-defined tool that executes a shell command.
#[derive(Clone)]
pub struct CustomTool {
    name: String,
    description: String,
    command: String,
}

impl CustomTool {
    pub fn new(def: &CustomToolDef) -> Self {
        Self {
            name: def.name.clone(),
            description: def.description.clone(),
            command: def.command.clone(),
        }
    }
}

impl Tool for CustomTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn execute(&self, args: &str) -> Result<String> {
        // Replace {{args}} or {{arg}} placeholder with the provided argument
        let cmd_str = if self.command.contains("{{args}}") {
            self.command.replace("{{args}}", args)
        } else if self.command.contains("{{arg}}") {
            self.command.replace("{{arg}}", args)
        } else {
            // If no placeholder, append args to the command
            if args.is_empty() {
                self.command.clone()
            } else {
                format!("{} {}", self.command, args)
            }
        };

        let output = Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute custom tool '{}': {}", self.name, e))?;

        let mut result = String::new();
        if !output.stdout.is_empty() {
            result.push_str(&String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            result.push_str(&format!(
                "\n[stderr]: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        if !output.status.success() {
            anyhow::bail!(
                "Custom tool '{}' exited with code {}:\n{}",
                self.name,
                output.status.code().unwrap_or(-1),
                result
            );
        }

        Ok(result.trim().to_string())
    }
}

/// Global cache for custom tools, loaded once at startup.
static CUSTOM_TOOLS: Mutex<Vec<Arc<dyn Tool>>> = Mutex::new(Vec::new());

/// Load custom tools from the config file.
pub fn load_custom_tools() -> Vec<Arc<dyn Tool>> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshark");
    let path = config_dir.join("custom_tools.toml");

    if !path.exists() {
        return Vec::new();
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read custom tools config: {}", e);
            return Vec::new();
        }
    };

    #[derive(Debug, Deserialize)]
    struct ToolFile {
        #[serde(default)]
        tool: Vec<CustomToolDef>,
    }

    let file: ToolFile = match toml::from_str(&content) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Failed to parse custom tools config: {}", e);
            return Vec::new();
        }
    };

    let tools: Vec<Arc<dyn Tool>> = file
        .tool
        .iter()
        .map(|def| {
            tracing::info!("Loaded custom tool: {}", def.name);
            Arc::new(CustomTool::new(def)) as Arc<dyn Tool>
        })
        .collect();

    // Cache them globally
    if let Ok(mut guard) = CUSTOM_TOOLS.lock() {
        guard.clear();
        guard.extend(tools.iter().cloned());
    }

    tools
}

/// Get cached custom tools.
pub fn get_custom_tools() -> Vec<Arc<dyn Tool>> {
    if let Ok(guard) = CUSTOM_TOOLS.lock() {
        guard.iter().cloned().collect()
    } else {
        Vec::new()
    }
}

/// Register custom tools into the global cache.
#[allow(dead_code)]
pub fn register_custom_tools(tools: Vec<Arc<dyn Tool>>) {
    if let Ok(mut guard) = CUSTOM_TOOLS.lock() {
        guard.clear();
        guard.extend(tools);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_tool_placeholder_replacement() {
        let tool = CustomTool {
            name: "echo".to_string(),
            description: "Echo args".to_string(),
            command: "echo {{args}}".to_string(),
        };

        let result = tool.execute("hello world").unwrap();
        assert!(result.contains("hello world"));
    }

    #[test]
    fn test_custom_tool_no_placeholder_appends_args() {
        let tool = CustomTool {
            name: "pwd".to_string(),
            description: "Print working directory".to_string(),
            command: "pwd".to_string(),
        };

        let result = tool.execute("").unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_custom_tool_failure() {
        let tool = CustomTool {
            name: "fail".to_string(),
            description: "Always fails".to_string(),
            command: "exit 1".to_string(),
        };

        assert!(tool.execute("").is_err());
    }
}
