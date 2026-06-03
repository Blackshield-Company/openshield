//! Plugin / Hook System — Register custom tools at runtime.
//!
//! Loads `.openshark/hooks/` directory for user-defined tool scripts.
//! Scripts are executable files named `<tool_name>.sh` or `<tool_name>.py`.
//! Plugin / Hook System — Load custom tools from `.openshark/hooks/`

#![allow(dead_code)]

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// A user-defined plugin tool.
#[derive(Debug, Clone)]
pub struct PluginTool {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub interpreter: Option<String>,
}

/// Registry of loaded plugins.
#[derive(Debug, Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginTool>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan `~/.config/openshark/hooks/` and `.openshark/hooks/` for plugin scripts.
    pub fn load_from_disk(&mut self) -> Result<usize> {
        let mut count = 0;

        let dirs = [
            dirs::config_dir().map(|d| d.join("openshark").join("hooks")),
            Some(PathBuf::from(".openshark/hooks")),
        ];

        for dir in dirs.iter().flatten() {
            if !dir.exists() {
                continue;
            }
            for entry in std::fs::read_dir(dir)?.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str());
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                if name.is_empty() || name.starts_with('.') {
                    continue;
                }

                let interpreter = match ext {
                    Some("sh") => Some("bash".to_string()),
                    Some("py") => Some("python3".to_string()),
                    Some("js") => Some("node".to_string()),
                    Some("rb") => Some("ruby".to_string()),
                    _ => None,
                };

                let description = Self::extract_description(&path).unwrap_or_else(|| {
                    format!("User-defined plugin: {}", name)
                });

                self.plugins.insert(
                    name.clone(),
                    PluginTool {
                        name,
                        description,
                        path,
                        interpreter,
                    },
                );
                count += 1;
            }
        }

        Ok(count)
    }

    fn extract_description(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        for line in content.lines().take(5) {
            if let Some(desc) = line.strip_prefix("# desc:") {
                return Some(desc.trim().to_string());
            }
            if let Some(desc) = line.strip_prefix("// desc:") {
                return Some(desc.trim().to_string());
            }
        }
        None
    }

    pub fn get(&self, name: &str) -> Option<&PluginTool> {
        self.plugins.get(name)
    }

    pub fn list(&self) -> Vec<&PluginTool> {
        self.plugins.values().collect()
    }

    pub fn create_scaffold(&self, name: &str) -> Result<PathBuf> {
        let hook_dir = PathBuf::from(".openshark/hooks");
        std::fs::create_dir_all(&hook_dir)?;
        let path = hook_dir.join(format!("{}.sh", name));
        let template = format!(
            "#!/bin/bash\n# desc: User-defined plugin: {{name}}\n# Args are passed via stdin\n\nread -r args\necho \"Running {{name}} with args: $args\"\n",
        );
        std::fs::write(&path, template)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(path)
    }

    pub fn enable(&mut self, _name: &str) -> Result<()> {
        // Placeholder: in full impl, would toggle a config flag
        Ok(())
    }

    pub fn disable(&mut self, _name: &str) -> Result<()> {
        // Placeholder: in full impl, would toggle a config flag
        Ok(())
    }

    /// Execute a plugin with the given arguments.
    pub async fn execute(&self, name: &str, args: &str) -> Result<String> {
        let plugin = self
            .plugins
            .get(name)
            .with_context(|| format!("Plugin '{}' not found", name))?;

        let mut cmd = tokio::process::Command::new(
            plugin.interpreter.as_deref().unwrap_or("bash"),
        );
        if plugin.interpreter.is_some() {
            cmd.arg(&plugin.path);
        } else {
            cmd.arg(&plugin.path);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(args.as_bytes()).await?;
        }

        let output = child.wait_with_output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            anyhow::bail!(
                "Plugin '{}' exited with {}: {}",
                name,
                output.status,
                if stderr.is_empty() { stdout } else { stderr }
            );
        }

        Ok(stdout)
    }
}

pub fn list_plugins_cli() {
    let mut registry = PluginRegistry::new();
    match registry.load_from_disk() {
        Ok(count) => {
            if count == 0 {
                println!("📭 No plugins found.");
                println!("Create one: openshark plugins create <name>");
            } else {
                println!("🔌 {} plugin(s) loaded:", count);
                for p in registry.list() {
                    println!("  - {}: {}", p.name, p.description);
                }
            }
        }
        Err(e) => eprintln!("❌ Failed to load plugins: {}", e),
    }
}
