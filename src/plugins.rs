use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Plugin manifest — every plugin has one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    /// Entry point: a shell command or script to run.
    pub entry: String,
    /// Hooks this plugin registers.
    #[serde(default)]
    pub hooks: Vec<HookType>,
    /// Configuration schema (key -> default value).
    #[serde(default)]
    pub config: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HookType {
    /// Called before a user message is processed.
    PreMessage,
    /// Called after an assistant response is received.
    PostResponse,
    /// Called when a tool is about to execute.
    PreTool,
    /// Called after a tool executes.
    PostTool,
    /// Called on session export.
    OnExport,
    /// Called on session import.
    OnImport,
}

/// A loaded plugin with its manifest and runtime state.
#[derive(Debug, Clone)]
pub struct Plugin {
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub enabled: bool,
}

/// Plugin registry — manages all loaded plugins.
#[derive(Debug, Clone)]
pub struct PluginRegistry {
    plugins: HashMap<String, Plugin>,
    plugin_dir: PathBuf,
}

impl PluginRegistry {
    /// Create a new registry and load all plugins from the default directory.
    pub fn new() -> Result<Self> {
        let plugin_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("openshark")
            .join("plugins");
        std::fs::create_dir_all(&plugin_dir)?;

        let mut registry = Self {
            plugins: HashMap::new(),
            plugin_dir,
        };
        registry.discover_and_load()?;
        Ok(registry)
    }

    /// Discover and load all plugins from the plugin directory.
    pub fn discover_and_load(&mut self) -> Result<usize> {
        self.plugins.clear();

        if !self.plugin_dir.exists() {
            return Ok(0);
        }

        let mut loaded = 0;
        for entry in std::fs::read_dir(&self.plugin_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let manifest_path = path.join("plugin.toml");
                if manifest_path.exists() {
                    match self.load_from_manifest(&manifest_path) {
                        Ok(plugin) => {
                            self.plugins.insert(plugin.manifest.name.clone(), plugin);
                            loaded += 1;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to load plugin from {:?}: {}", path, e);
                        }
                    }
                }
            }
        }

        Ok(loaded)
    }

    fn load_from_manifest(&self, path: &Path) -> Result<Plugin> {
        let content = std::fs::read_to_string(path)
            .context("Failed to read plugin manifest")?;
        let manifest: PluginManifest = toml::from_str(&content)
            .context("Failed to parse plugin manifest")?;

        Ok(Plugin {
            manifest,
            path: path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            enabled: true,
        })
    }

    /// Get a plugin by name.
    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.get(name)
    }

    /// Get a mutable plugin by name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Plugin> {
        self.plugins.get_mut(name)
    }

    /// List all loaded plugins.
    pub fn list(&self) -> Vec<&Plugin> {
        self.plugins.values().collect()
    }

    /// List enabled plugins.
    pub fn enabled_plugins(&self) -> Vec<&Plugin> {
        self.plugins.values().filter(|p| p.enabled).collect()
    }

    /// Enable a plugin.
    pub fn enable(&mut self, name: &str) -> Result<()> {
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.enabled = true;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Plugin '{}' not found", name))
        }
    }

    /// Disable a plugin.
    pub fn disable(&mut self, name: &str) -> Result<()> {
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.enabled = false;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Plugin '{}' not found", name))
        }
    }

    /// Execute a plugin's entry point with optional input data.
    /// Returns the stdout output.
    pub fn execute(&self, name: &str, input: Option<&str>) -> Result<String> {
        let plugin = self.plugins.get(name)
            .ok_or_else(|| anyhow::anyhow!("Plugin '{}' not found", name))?;

        if !plugin.enabled {
            return Err(anyhow::anyhow!("Plugin '{}' is disabled", name));
        }

        let entry_path = plugin.path.join(&plugin.manifest.entry);
        let output = if entry_path.exists() {
            // It's a file — execute it
            let mut cmd = std::process::Command::new(&entry_path);
            if let Some(data) = input {
                cmd.arg(data);
            }
            cmd.current_dir(&plugin.path)
                .output()
                .context("Failed to execute plugin")?
        } else {
            // Treat as a shell command
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("-c")
                .arg(&plugin.manifest.entry)
                .current_dir(&plugin.path);
            if let Some(data) = input {
                cmd.arg(data);
            }
            cmd.output()
                .context("Failed to execute plugin shell command")?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Plugin failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Run all plugins that registered a given hook.
    pub fn run_hook(&self, hook: HookType, input: Option<&str>) -> Vec<(String, Result<String>)> {
        let mut results = Vec::new();
        for plugin in self.enabled_plugins() {
            if plugin.manifest.hooks.contains(&hook) {
                let result = self.execute(&plugin.manifest.name, input);
                results.push((plugin.manifest.name.clone(), result));
            }
        }
        results
    }

    /// Create a new plugin scaffold in the plugin directory.
    pub fn create_scaffold(&self, name: &str) -> Result<PathBuf> {
        let plugin_path = self.plugin_dir.join(name);
        std::fs::create_dir_all(&plugin_path)?;

        let manifest = PluginManifest {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: format!("{} plugin for OpenShark", name),
            author: None,
            entry: "plugin.sh".to_string(),
            hooks: vec![HookType::PreMessage, HookType::PostResponse],
            config: HashMap::new(),
        };

        let manifest_toml = toml::to_string_pretty(&manifest)?;
        std::fs::write(plugin_path.join("plugin.toml"), manifest_toml)?;

        let shell_script = format!(
            "#!/bin/bash\n# {} plugin for OpenShark\n# This script receives data via stdin or $1\n\necho \"Hello from {} plugin!\"\n",
            name, name
        );
        std::fs::write(plugin_path.join("plugin.sh"), shell_script)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(plugin_path.join("plugin.sh"))?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(plugin_path.join("plugin.sh"), perms)?;
        }

        Ok(plugin_path)
    }
}

/// CLI helper: list all plugins with their status.
pub fn list_plugins_cli() {
    match PluginRegistry::new() {
        Ok(registry) => {
            let plugins = registry.list();
            if plugins.is_empty() {
                println!("🦈 No plugins installed.");
                println!("   Create one with: openshark plugins create <name>");
                return;
            }

            println!("🦈 OpenShark Plugins ({} total)\n", plugins.len());
            for plugin in plugins {
                let status = if plugin.enabled {
                    "● enabled"
                } else {
                    "○ disabled"
                };
                println!(
                    "  {} {} v{} — {}",
                    status,
                    plugin.manifest.name,
                    plugin.manifest.version,
                    plugin.manifest.description
                );
                println!(
                    "    Entry: {} | Hooks: {:?}",
                    plugin.manifest.entry,
                    plugin.manifest.hooks
                );
            }
        }
        Err(e) => println!("❌ Failed to load plugins: {}", e),
    }
}
