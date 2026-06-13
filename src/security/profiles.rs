//! Permission Profiles — Switchable security presets.
//!
//! Pre-defined profiles that configure tool permissions, risk thresholds,
//! and sandbox settings as a cohesive unit. Users switch profiles via
//! `/profile <name>` instead of toggling individual settings.
//!
//! Built-in profiles:
//!   - coding:    Full tool access, High auto-approve (default)
//!   - review:    Read-only + git, Medium auto-approve
//!   - safe:      Ask for everything, Low auto-approve
//!   - yolo:      Allow everything, Critical auto-approve (dangerous)
//!   - readonly:  fs read + search only, None auto-approve

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::security::{PermissionLevel, RiskLevel, SecurityConfig};

/// A named permission profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionProfile {
    pub name: String,
    pub description: String,
    /// Tool name -> PermissionLevel overrides (empty = use defaults).
    pub tool_permissions: HashMap<String, PermissionLevel>,
    /// Auto-approve risk threshold.
    pub auto_approve_risk_level: RiskLevel,
    /// Whether sudo is enabled.
    pub sudo_enabled: bool,
    /// Whether to allow escaping working directory.
    pub allow_escape_working_dir: bool,
    /// Whether PII redaction is enabled.
    pub pii_redaction_enabled: bool,
    /// Whether prompt injection detection is enabled.
    pub prompt_injection_detection_enabled: bool,
    /// Maximum output bytes to send to model.
    pub max_model_output_bytes: usize,
}

/// Profile registry with built-ins and custom profiles.
#[derive(Clone)]
pub struct ProfileRegistry {
    profiles: HashMap<String, PermissionProfile>,
    active: String,
}

impl Default for ProfileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileRegistry {
    pub fn new() -> Self {
        let mut profiles = HashMap::new();

        profiles.insert(
            "coding".to_string(),
            PermissionProfile {
                name: "coding".to_string(),
                description: "Full coding tool access. Auto-approves up to High risk.".to_string(),
                tool_permissions: coding_tool_permissions(),
                auto_approve_risk_level: RiskLevel::High,
                sudo_enabled: true,
                allow_escape_working_dir: false,
                pii_redaction_enabled: true,
                prompt_injection_detection_enabled: true,
                max_model_output_bytes: 32768,
            },
        );

        profiles.insert(
            "review".to_string(),
            PermissionProfile {
                name: "review".to_string(),
                description: "Read-only review mode. Auto-approves up to Medium risk.".to_string(),
                tool_permissions: review_tool_permissions(),
                auto_approve_risk_level: RiskLevel::Medium,
                sudo_enabled: false,
                allow_escape_working_dir: false,
                pii_redaction_enabled: true,
                prompt_injection_detection_enabled: true,
                max_model_output_bytes: 65536,
            },
        );

        profiles.insert(
            "safe".to_string(),
            PermissionProfile {
                name: "safe".to_string(),
                description: "Ask before every tool call. Maximum caution.".to_string(),
                tool_permissions: safe_tool_permissions(),
                auto_approve_risk_level: RiskLevel::Low,
                sudo_enabled: false,
                allow_escape_working_dir: false,
                pii_redaction_enabled: true,
                prompt_injection_detection_enabled: true,
                max_model_output_bytes: 16384,
            },
        );

        profiles.insert(
            "yolo".to_string(),
            PermissionProfile {
                name: "yolo".to_string(),
                description: "DANGER: Auto-approves everything including Critical risk."
                    .to_string(),
                tool_permissions: yolo_tool_permissions(),
                auto_approve_risk_level: RiskLevel::Critical,
                sudo_enabled: true,
                allow_escape_working_dir: true,
                pii_redaction_enabled: false,
                prompt_injection_detection_enabled: false,
                max_model_output_bytes: 131072,
            },
        );

        profiles.insert(
            "readonly".to_string(),
            PermissionProfile {
                name: "readonly".to_string(),
                description: "Read-only filesystem access. No edits, no terminal.".to_string(),
                tool_permissions: readonly_tool_permissions(),
                auto_approve_risk_level: RiskLevel::None,
                sudo_enabled: false,
                allow_escape_working_dir: false,
                pii_redaction_enabled: true,
                prompt_injection_detection_enabled: true,
                max_model_output_bytes: 32768,
            },
        );

        Self {
            profiles,
            active: "coding".to_string(),
        }
    }

    /// Get the active profile name.
    pub fn active(&self) -> &str {
        &self.active
    }

    /// Get a profile by name.
    pub fn get(&self, name: &str) -> Option<&PermissionProfile> {
        self.profiles.get(name)
    }

    /// List all available profile names.
    pub fn list(&self) -> Vec<&String> {
        self.profiles.keys().collect()
    }

    /// Switch to a profile by name.
    pub fn switch(&mut self, name: &str) -> Result<(), String> {
        if !self.profiles.contains_key(name) {
            return Err(format!(
                "Unknown profile '{}'. Available: {}",
                name,
                self.profiles.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        self.active = name.to_string();
        Ok(())
    }

    /// Apply the active profile to a SecurityConfig, returning a modified copy.
    pub fn apply_to_config(&self, config: &SecurityConfig) -> SecurityConfig {
        let profile = match self.profiles.get(&self.active) {
            Some(p) => p,
            None => return config.clone(),
        };

        let mut new_config = config.clone();
        new_config.tool_permissions = profile.tool_permissions.clone();
        new_config.auto_approve_risk_level = profile.auto_approve_risk_level.clone();
        new_config.sudo.enabled = profile.sudo_enabled;
        new_config.allow_escape_working_dir = profile.allow_escape_working_dir;
        new_config.pii_redaction_enabled = profile.pii_redaction_enabled;
        new_config.prompt_injection_detection_enabled = profile.prompt_injection_detection_enabled;
        new_config.max_model_output_bytes = profile.max_model_output_bytes;
        new_config
    }

    /// Get a summary of the active profile for display.
    pub fn active_summary(&self) -> String {
        match self.profiles.get(&self.active) {
            Some(p) => format!(
                "Profile: {} — {}\n  Auto-approve: {:?} | Sudo: {} | Escape: {} | PII: {}",
                p.name,
                p.description,
                p.auto_approve_risk_level,
                if p.sudo_enabled { "on" } else { "off" },
                if p.allow_escape_working_dir {
                    "on"
                } else {
                    "off"
                },
                if p.pii_redaction_enabled { "on" } else { "off" }
            ),
            None => format!("Profile: {} (not found)", self.active),
        }
    }
}

fn coding_tool_permissions() -> HashMap<String, PermissionLevel> {
    let mut m = HashMap::new();
    m.insert("fs".to_string(), PermissionLevel::Allow);
    m.insert("terminal".to_string(), PermissionLevel::Allow);
    m.insert("git".to_string(), PermissionLevel::Allow);
    m.insert("search".to_string(), PermissionLevel::Allow);
    m.insert("edit".to_string(), PermissionLevel::Allow);
    m.insert("lsp".to_string(), PermissionLevel::Allow);
    m.insert("refactor".to_string(), PermissionLevel::Allow);
    m.insert("test".to_string(), PermissionLevel::Allow);
    m.insert("guardian".to_string(), PermissionLevel::Allow);
    m.insert("repo_map".to_string(), PermissionLevel::Allow);
    m.insert("checkpoint".to_string(), PermissionLevel::Allow);
    m
}

fn review_tool_permissions() -> HashMap<String, PermissionLevel> {
    let mut m = HashMap::new();
    m.insert("fs".to_string(), PermissionLevel::Ask);
    m.insert("terminal".to_string(), PermissionLevel::Deny);
    m.insert("git".to_string(), PermissionLevel::Allow);
    m.insert("search".to_string(), PermissionLevel::Allow);
    m.insert("edit".to_string(), PermissionLevel::Deny);
    m.insert("lsp".to_string(), PermissionLevel::Allow);
    m.insert("refactor".to_string(), PermissionLevel::Deny);
    m.insert("test".to_string(), PermissionLevel::Ask);
    m.insert("guardian".to_string(), PermissionLevel::Allow);
    m.insert("repo_map".to_string(), PermissionLevel::Allow);
    m.insert("checkpoint".to_string(), PermissionLevel::Deny);
    m
}

fn safe_tool_permissions() -> HashMap<String, PermissionLevel> {
    let mut m = HashMap::new();
    m.insert("fs".to_string(), PermissionLevel::Ask);
    m.insert("terminal".to_string(), PermissionLevel::Ask);
    m.insert("git".to_string(), PermissionLevel::Ask);
    m.insert("search".to_string(), PermissionLevel::Ask);
    m.insert("edit".to_string(), PermissionLevel::Ask);
    m.insert("lsp".to_string(), PermissionLevel::Ask);
    m.insert("refactor".to_string(), PermissionLevel::Ask);
    m.insert("test".to_string(), PermissionLevel::Ask);
    m.insert("guardian".to_string(), PermissionLevel::Ask);
    m.insert("repo_map".to_string(), PermissionLevel::Ask);
    m.insert("checkpoint".to_string(), PermissionLevel::Ask);
    m
}

fn yolo_tool_permissions() -> HashMap<String, PermissionLevel> {
    let mut m = HashMap::new();
    m.insert("fs".to_string(), PermissionLevel::Allow);
    m.insert("terminal".to_string(), PermissionLevel::Allow);
    m.insert("git".to_string(), PermissionLevel::Allow);
    m.insert("search".to_string(), PermissionLevel::Allow);
    m.insert("edit".to_string(), PermissionLevel::Allow);
    m.insert("lsp".to_string(), PermissionLevel::Allow);
    m.insert("refactor".to_string(), PermissionLevel::Allow);
    m.insert("test".to_string(), PermissionLevel::Allow);
    m.insert("guardian".to_string(), PermissionLevel::Allow);
    m.insert("repo_map".to_string(), PermissionLevel::Allow);
    m.insert("checkpoint".to_string(), PermissionLevel::Allow);
    m
}

fn readonly_tool_permissions() -> HashMap<String, PermissionLevel> {
    let mut m = HashMap::new();
    m.insert("fs".to_string(), PermissionLevel::Ask);
    m.insert("terminal".to_string(), PermissionLevel::Deny);
    m.insert("git".to_string(), PermissionLevel::Deny);
    m.insert("search".to_string(), PermissionLevel::Allow);
    m.insert("edit".to_string(), PermissionLevel::Deny);
    m.insert("lsp".to_string(), PermissionLevel::Allow);
    m.insert("refactor".to_string(), PermissionLevel::Deny);
    m.insert("test".to_string(), PermissionLevel::Deny);
    m.insert("guardian".to_string(), PermissionLevel::Allow);
    m.insert("repo_map".to_string(), PermissionLevel::Allow);
    m.insert("checkpoint".to_string(), PermissionLevel::Deny);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_profiles() {
        let registry = ProfileRegistry::new();
        assert!(registry.get("coding").is_some());
        assert!(registry.get("safe").is_some());
        assert!(registry.get("yolo").is_some());
        assert!(registry.get("readonly").is_some());
        assert!(registry.get("review").is_some());
    }

    #[test]
    fn test_switch_profile() {
        let mut registry = ProfileRegistry::new();
        assert_eq!(registry.active(), "coding");
        registry.switch("safe").unwrap();
        assert_eq!(registry.active(), "safe");
    }

    #[test]
    fn test_switch_unknown() {
        let mut registry = ProfileRegistry::new();
        assert!(registry.switch("nope").is_err());
    }

    #[test]
    fn test_apply_to_config() {
        let registry = ProfileRegistry::new();
        let base = SecurityConfig::default();

        // Coding should allow fs
        let coding = registry.apply_to_config(&base);
        assert_eq!(coding.get_tool_permission("fs"), PermissionLevel::Allow);

        // Readonly should deny terminal
        let mut reg = ProfileRegistry::new();
        reg.switch("readonly").unwrap();
        let readonly = reg.apply_to_config(&base);
        assert_eq!(
            readonly.get_tool_permission("terminal"),
            PermissionLevel::Deny
        );
    }
}
