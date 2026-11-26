//! `[modules]` table in `config.toml`.
//!
//! Cargo features decide whether a module is compiled in. These structs
//! decide whether a compiled module is enabled at runtime.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// On/off switch for a module that has no extra settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleToggle {
    /// When false, the host does not install hooks or routes, and
    /// mutating CLI subcommands exit with code 2.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for ModuleToggle {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl ModuleToggle {
    /// Whether the module should be active at runtime.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Armory-specific settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmoryModuleConfig {
    /// Runtime enable switch.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// When true, unsigned hook scripts and MCP commands are refused at load.
    /// Hand-copied skills still load, with a warning.
    #[serde(default)]
    pub require_signatures: bool,
}

impl Default for ArmoryModuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_signatures: false,
        }
    }
}

impl ArmoryModuleConfig {
    /// Whether the module should be active at runtime.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Guard-specific settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardModuleConfig {
    /// Runtime enable switch.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Policy file. If missing, Guard installs no hook and does not
    /// change existing security behaviour.
    #[serde(default = "default_guard_policy")]
    pub policy: PathBuf,
}

fn default_guard_policy() -> PathBuf {
    PathBuf::from("~/.config/openshield/guard.toml")
}

impl Default for GuardModuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            policy: default_guard_policy(),
        }
    }
}

impl GuardModuleConfig {
    /// Whether the module should be active at runtime.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// The `[modules]` table. Missing keys use the defaults (enabled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModulesConfig {
    /// Session replay and redacted export.
    #[serde(default)]
    pub replay: ModuleToggle,
    /// Eval arena (Heatrun). Reserved until the eval crate lands.
    #[serde(default)]
    pub eval: ModuleToggle,
    /// Signed skill / hook / MCP registry.
    #[serde(default)]
    pub armory: ArmoryModuleConfig,
    /// Policy proxy (Gatehouse).
    #[serde(default)]
    pub guard: GuardModuleConfig,
}

impl ModulesConfig {
    /// Parse a TOML snippet that contains a `[modules]` table, or a
    /// fragment that is itself the table.
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(default)]
            modules: ModulesConfig,
        }
        if s.trim().is_empty() {
            return Ok(Self::default());
        }
        if s.contains("[modules") {
            Ok(toml::from_str::<Wrapper>(s)?.modules)
        } else {
            toml::from_str(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_enabled() {
        let m = ModulesConfig::default();
        assert!(m.replay.is_enabled());
        assert!(m.eval.is_enabled());
        assert!(m.armory.is_enabled());
        assert!(!m.armory.require_signatures);
        assert!(m.guard.is_enabled());
        assert_eq!(
            m.guard.policy,
            PathBuf::from("~/.config/openshield/guard.toml")
        );
    }

    #[test]
    fn missing_table_is_default() {
        let m = ModulesConfig::from_toml_str("").unwrap();
        assert!(m.replay.is_enabled());
    }

    #[test]
    fn parses_nested_tables() {
        let raw = r#"
[modules.replay]
enabled = false

[modules.armory]
enabled = true
require_signatures = true

[modules.guard]
enabled = true
policy = "/tmp/guard.toml"
"#;
        let m = ModulesConfig::from_toml_str(raw).unwrap();
        assert!(!m.replay.is_enabled());
        assert!(m.eval.is_enabled());
        assert!(m.armory.require_signatures);
        assert_eq!(m.guard.policy, PathBuf::from("/tmp/guard.toml"));
    }

    #[test]
    fn partial_table_keeps_other_defaults() {
        let m = ModulesConfig::from_toml_str("[modules.replay]\nenabled = false\n").unwrap();
        assert!(!m.replay.is_enabled());
        assert!(m.armory.is_enabled());
        assert!(m.guard.is_enabled());
    }
}
