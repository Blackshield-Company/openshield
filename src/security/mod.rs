//! OpenShark Security Architecture
//!
//! Layered security model:
//!   L1 - Infrastructure Isolation: sandbox/working dir, process isolation
//!   L2 - Identity & Access: scoped credentials, zero-trust, temp tokens
//!   L3 - Data Protection: PII redaction, encryption, sensitive data detection
//!   L4 - Application Guardrails: tool permissions, prompt injection detection,
//!        output validation, human approval gates
//!
//! All security decisions flow through the SecurityEngine which coordinates
//! across layers. No tool executes without passing through the security gate.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::info;

pub mod guardrails;
pub mod identity;
pub mod pii;
pub mod profiles;
pub mod sandbox;

pub use guardrails::*;
pub use identity::*;
pub use pii::*;
pub use profiles::*;
pub use sandbox::*;

/// Security configuration stored in user's config directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub version: String,
    /// Working directory restriction. None = current dir, Some = enforced.
    pub working_directory: Option<PathBuf>,
    /// Whether to allow commands outside working directory.
    pub allow_escape_working_dir: bool,
    /// Sudo configuration.
    pub sudo: SudoConfig,
    /// Tool permission levels.
    pub tool_permissions: HashMap<String, PermissionLevel>,
    /// Sensitive paths that require approval to access.
    pub sensitive_paths: Vec<String>,
    /// User-configured allowed paths (from FilesystemConfig). Empty = no restriction.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Patterns for sensitive data detection.
    pub sensitive_patterns: Vec<String>,
    /// Maximum output size to send to model (bytes).
    pub max_model_output_bytes: usize,
    /// Whether PII redaction is enabled.
    pub pii_redaction_enabled: bool,
    /// Whether prompt injection detection is enabled.
    pub prompt_injection_detection_enabled: bool,
    /// Auto-approve tools below this risk level.
    pub auto_approve_risk_level: RiskLevel,
    /// Identity and access config.
    pub identity: IdentityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SudoConfig {
    /// Whether sudo commands are allowed at all.
    pub enabled: bool,
    /// Whether to persist sudo password (masked in config).
    pub persist_password: bool,
    /// The sudo password (stored as encrypted/masked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
    /// Commands that always require approval even with persisted password.
    pub always_approve_commands: Vec<String>,
    /// Timeout for sudo approval (seconds).
    pub approval_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    None,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionLevel {
    /// Tool can run without approval.
    Allow,
    /// Tool requires human approval before execution.
    Ask,
    /// Tool is completely disabled.
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    /// Whether zero-trust mode is enabled.
    pub zero_trust_enabled: bool,
    /// Session-scoped credential TTL in seconds.
    pub credential_ttl_secs: u64,
    /// Maximum concurrent sessions per identity.
    pub max_concurrent_sessions: usize,
    /// Allowed API endpoints (empty = all).
    pub allowed_endpoints: Vec<String>,
    /// Blocked API endpoints.
    pub blocked_endpoints: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        let mut tool_permissions = HashMap::new();
        // CODING MODE: Allow all coding tools to execute without approval
        // Only destructive system operations are blocked
        tool_permissions.insert("fs".to_string(), PermissionLevel::Allow);
        tool_permissions.insert("terminal".to_string(), PermissionLevel::Allow);
        tool_permissions.insert("git".to_string(), PermissionLevel::Allow);
        tool_permissions.insert("search".to_string(), PermissionLevel::Allow);
        tool_permissions.insert("edit".to_string(), PermissionLevel::Allow);
        tool_permissions.insert("lsp".to_string(), PermissionLevel::Allow);
        tool_permissions.insert("refactor".to_string(), PermissionLevel::Allow);
        tool_permissions.insert("test".to_string(), PermissionLevel::Allow);

        Self {
            version: crate::VERSION.to_string(),
            working_directory: None,
            allow_escape_working_dir: false,
            sudo: SudoConfig {
                enabled: true,
                persist_password: false,
                password_hash: None,
                always_approve_commands: vec![
                    "rm".to_string(),
                    "dd".to_string(),
                    "mkfs".to_string(),
                    "fdisk".to_string(),
                ],
                approval_timeout_secs: 300,
            },
            tool_permissions,
            sensitive_paths: vec![
                "/etc/shadow".to_string(),
                "/etc/passwd".to_string(),
                "~/.ssh".to_string(),
                "~/.gnupg".to_string(),
                "~/.config/openshark".to_string(),
                "~/.hermes".to_string(),
            ],
            allowed_paths: vec![],
            sensitive_patterns: vec![
                r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b".to_string(),
                r"\b(?:\d{4}-?){3}\d{4}\b".to_string(),
                r"\b\d{3}-\d{2}-\d{4}\b".to_string(),
                r"sk-[a-zA-Z0-9]{48}".to_string(),
                r"ghp_[a-zA-Z0-9]{36}".to_string(),
                r"AKIA[0-9A-Z]{16}".to_string(),
            ],
            max_model_output_bytes: 32768,
            pii_redaction_enabled: true,
            prompt_injection_detection_enabled: true,
            // FULL-SEND MODE: Auto-approve up to High risk (mkdir, curl, redirects, ssh)
            // Only Critical (rm -rf, mkfs, fdisk, format) requires approval
            auto_approve_risk_level: RiskLevel::High,
            identity: IdentityConfig {
                zero_trust_enabled: true,
                credential_ttl_secs: 3600,
                max_concurrent_sessions: 5,
                allowed_endpoints: vec![],
                blocked_endpoints: vec![],
            },
        }
    }
}

impl Default for SudoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            persist_password: false,
            password_hash: None,
            always_approve_commands: vec![
                "rm".to_string(),
                "dd".to_string(),
                "mkfs".to_string(),
                "fdisk".to_string(),
            ],
            approval_timeout_secs: 300,
        }
    }
}

impl SecurityConfig {
    pub fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("No config directory found")?
            .join("openshark");
        let path = config_dir.join("security.toml");

        let mut config = if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let config: SecurityConfig =
                toml::from_str(&content).context("Failed to parse security.toml")?;
            config
        } else {
            let config = SecurityConfig::default();
            config.save()?;
            config
        };

        // Sync allowed_paths from main config.toml
        let main_config_path = config_dir.join("config.toml");
        if main_config_path.exists()
            && let Ok(content) = std::fs::read_to_string(&main_config_path)
                && let Ok(main_config) = toml::from_str::<crate::config::Config>(&content)
                    && !main_config.filesystem.allowed_paths.is_empty() {
                        config.allowed_paths = main_config.filesystem.allowed_paths.clone();
                    }

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_dir = dirs::config_dir()
            .context("No config directory found")?
            .join("openshark");
        std::fs::create_dir_all(&config_dir)?;

        let path = config_dir.join("security.toml");
        // Mask password before saving
        let mut config = self.clone();
        if config.sudo.password_hash.is_some() {
            config.sudo.password_hash = Some("****".to_string());
        }
        let content =
            toml::to_string_pretty(&config).context("Failed to serialize security config")?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn get_tool_permission(&self, tool_name: &str) -> PermissionLevel {
        self.tool_permissions
            .get(tool_name)
            .cloned()
            .unwrap_or(PermissionLevel::Ask)
    }
}

/// The central security engine that coordinates all security layers.
#[derive(Clone)]
pub struct SecurityEngine {
    /// Security configuration.
    pub config: SecurityConfig,
    /// Tracks approved sudo sessions (command -> expiry time).
    sudo_sessions: Arc<Mutex<HashMap<String, std::time::Instant>>>,
    /// Tracks active tool executions for audit.
    audit_log: Arc<Mutex<Vec<AuditEntry>>>,
    /// Sandbox manager for working directory isolation.
    sandbox: Sandbox,
    /// PII detector for data protection.
    pub pii_detector: PiiDetector,
    /// Guardrails for prompt/output validation.
    guardrails: Guardrails,
    /// Identity manager for zero-trust.
    #[allow(dead_code)]
    identity_manager: IdentityManager,
    /// Permission profile registry for switching presets.
    #[allow(dead_code)]
    pub profile_registry: Arc<Mutex<ProfileRegistry>>,
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub action: String,
    pub tool: String,
    #[allow(dead_code)]
    pub args: String,
    pub approved: bool,
    pub risk_level: RiskLevel,
    pub reason: String,
}

/// Result of a security check.
#[derive(Debug, Clone)]
pub enum SecurityDecision {
    /// Execution is approved.
    Allow,
    /// Execution requires human approval.
    RequireApproval {
        reason: String,
        risk_level: RiskLevel,
    },
    /// Execution is denied.
    Deny { reason: String },
}

impl SecurityEngine {
    pub fn new(config: SecurityConfig) -> Result<Self> {
        let mut sandbox = Sandbox::new(config.working_directory.clone())?;
        sandbox.set_allowed_paths(config.allowed_paths.clone());
        let pii_detector = PiiDetector::new(&config.sensitive_patterns);
        let guardrails = Guardrails::new(config.prompt_injection_detection_enabled);
        let identity_manager = IdentityManager::new(config.identity.clone());

        Ok(Self {
            config,
            sudo_sessions: Arc::new(Mutex::new(HashMap::new())),
            audit_log: Arc::new(Mutex::new(Vec::new())),
            sandbox,
            pii_detector,
            guardrails,
            identity_manager,
            profile_registry: Arc::new(Mutex::new(ProfileRegistry::new())),
        })
    }

    /// Main security gate: checks a tool call before execution.
    /// When autonomous_mode is true, elevates auto-approve threshold to High
    /// so the model can curl, redirect output, etc. without blocking.
    pub fn check_tool_call(&self, tool_name: &str, args: &str) -> SecurityDecision {
        self.check_tool_call_with_mode(tool_name, args, false)
    }

    /// Check tool call with explicit autonomous mode override.
    pub fn check_tool_call_with_mode(
        &self,
        tool_name: &str,
        args: &str,
        autonomous_mode: bool,
    ) -> SecurityDecision {
        // L1: Check sandbox / working directory restrictions
        if let Err(reason) = self.sandbox.validate_path(tool_name, args) {
            return SecurityDecision::Deny { reason };
        }

        // L2: Check tool permissions
        match self.config.get_tool_permission(tool_name) {
            PermissionLevel::Deny => {
                return SecurityDecision::Deny {
                    reason: format!("Tool '{}' is disabled by security policy", tool_name),
                };
            }
            PermissionLevel::Ask => {
                // Continue to risk assessment
            }
            PermissionLevel::Allow => {
                // Still check for risky patterns
            }
        }

        // L3: Check for sudo commands — NEVER bypassed, even in autonomous mode
        if let Some(sudo_check) = self.check_sudo(tool_name, args) {
            return sudo_check;
        }

        // L4: Check sensitive path access — NEVER bypassed
        if let Some(sensitive) = self.check_sensitive_paths(tool_name, args) {
            return sensitive;
        }

        // L5: Assess risk level
        let risk = self.assess_risk(tool_name, args);
        let threshold = if autonomous_mode {
            RiskLevel::High
        } else {
            self.config.auto_approve_risk_level.clone()
        };
        if risk > threshold {
            return SecurityDecision::RequireApproval {
                reason: format!(
                    "Risk level '{:?}' exceeds auto-approve threshold (autonomous={})",
                    risk, autonomous_mode
                ),
                risk_level: risk,
            };
        }

        // L6: Check for PII in arguments — still checked in autonomous mode
        if self.config.pii_redaction_enabled {
            let pii_findings = self.pii_detector.scan(args);
            if !pii_findings.is_empty() {
                return SecurityDecision::RequireApproval {
                    reason: format!("Potential PII detected in arguments: {:?}", pii_findings),
                    risk_level: RiskLevel::High,
                };
            }
        }

        SecurityDecision::Allow
    }

    /// Validate and sanitize output before sending to model.
    pub fn sanitize_output(&self, tool_name: &str, output: &str) -> String {
        let mut sanitized = output.to_string();

        // Truncate if too large
        if sanitized.len() > self.config.max_model_output_bytes {
            let truncated = crate::utils::truncate_str(&sanitized, self.config.max_model_output_bytes);
            sanitized = format!(
                "{}\n\n[Output truncated: {} bytes total, showing first {}]",
                truncated,
                output.len(),
                self.config.max_model_output_bytes
            );
        }

        // Redact PII
        if self.config.pii_redaction_enabled {
            sanitized = self.pii_detector.redact(&sanitized);
        }

        // Redact API keys and secrets
        sanitized = redact_secrets(&sanitized);

        // Tool-specific sanitization
        sanitized = match tool_name {
            "terminal" | "fs" => self.sanitize_filesystem_output(&sanitized),
            "git" => self.sanitize_git_output(&sanitized),
            _ => sanitized,
        };

        sanitized
    }

    /// Check user input for prompt injection attempts.
    pub fn check_prompt_injection(&self, input: &str) -> Option<String> {
        if !self.config.prompt_injection_detection_enabled {
            return None;
        }
        self.guardrails.detect_injection(input)
    }

    /// Record an audit entry.
    pub fn audit(&self, tool: &str, args: &str, approved: bool, risk: RiskLevel, reason: &str) {
        let entry = AuditEntry {
            timestamp: chrono::Utc::now(),
            action: "tool_execution".to_string(),
            tool: tool.to_string(),
            args: args.to_string(),
            approved,
            risk_level: risk,
            reason: reason.to_string(),
        };
        if let Ok(mut log) = self.audit_log.lock() {
            log.push(entry);
            // Keep last 1000 entries
            if log.len() > 1000 {
                log.remove(0);
            }
        }
    }

    /// Get recent audit entries.
    pub fn get_audit_log(&self, limit: usize) -> Vec<AuditEntry> {
        if let Ok(log) = self.audit_log.lock() {
            log.iter().rev().take(limit).cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Check if a sudo command needs approval.
    fn check_sudo(&self, tool_name: &str, args: &str) -> Option<SecurityDecision> {
        if tool_name != "terminal" {
            return None;
        }

        let args_lower = args.to_lowercase();
        if !args_lower.contains("sudo") {
            return None;
        }

        if !self.config.sudo.enabled {
            return Some(SecurityDecision::Deny {
                reason: "Sudo commands are disabled by security policy".to_string(),
            });
        }

        // Check if command is in always-approve list
        for cmd in &self.config.sudo.always_approve_commands {
            if args_lower.contains(cmd) {
                return Some(SecurityDecision::RequireApproval {
                    reason: format!("Sudo command '{}' requires explicit approval", cmd),
                    risk_level: RiskLevel::Critical,
                });
            }
        }

        // Check for active sudo session
        if let Ok(sessions) = self.sudo_sessions.lock()
            && let Some(expiry) = sessions.get(args)
                && std::time::Instant::now() < *expiry {
                    return None; // Session still valid
                }

        Some(SecurityDecision::RequireApproval {
            reason: "Sudo command requires approval".to_string(),
            risk_level: RiskLevel::High,
        })
    }

    /// Check if tool args touch sensitive paths.
    fn check_sensitive_paths(&self, tool_name: &str, args: &str) -> Option<SecurityDecision> {
        let tools_that_access_files = ["fs", "terminal", "edit"];
        if !tools_that_access_files.contains(&tool_name) {
            return None;
        }

        for sensitive in &self.config.sensitive_paths {
            let expanded = shellexpand::tilde(sensitive).to_string();
            if args.contains(&expanded) || args.contains(sensitive) {
                return Some(SecurityDecision::RequireApproval {
                    reason: format!("Access to sensitive path '{}' requires approval", sensitive),
                    risk_level: RiskLevel::High,
                });
            }
        }

        None
    }

    /// Assess risk level of a tool call.
    fn assess_risk(&self, tool_name: &str, args: &str) -> RiskLevel {
        let args_lower = args.to_lowercase();

        // Critical: destructive operations
        let critical_patterns = ["rm -rf", "dd if=", "mkfs", "fdisk", "format"];
        for pat in &critical_patterns {
            if args_lower.contains(pat) {
                return RiskLevel::Critical;
            }
        }

        // High: write operations, network calls
        let high_patterns = [">", ">>", "curl", "wget", "scp", "ssh", "nc -"];
        for pat in &high_patterns {
            if args_lower.contains(pat) {
                return RiskLevel::High;
            }
        }

        // Medium: git push, package installation
        let medium_patterns = ["git push", "pip install", "npm install", "cargo install"];
        for pat in &medium_patterns {
            if args_lower.contains(pat) {
                return RiskLevel::Medium;
            }
        }

        // Low: read operations
        match tool_name {
            "fs" if args_lower.starts_with("read") || args_lower.starts_with("list") => {
                RiskLevel::Low
            }
            "search" => RiskLevel::Low,
            "git" if args_lower.starts_with("status") || args_lower.starts_with("log") => {
                RiskLevel::Low
            }
            _ => RiskLevel::None,
        }
    }

    fn sanitize_filesystem_output(&self, output: &str) -> String {
        // Remove absolute home paths
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        output.replace(&home, "~")
    }

    fn sanitize_git_output(&self, output: &str) -> String {
        // Redact remote URLs that might contain tokens
        let mut result = output.to_string();
        if let Ok(re) = regex::Regex::new(r"https://[^@\s]+@([^\s]+)") {
            result = re.replace_all(&result, "https://***@$1").to_string();
        }
        result
    }

    /// Grant temporary sudo approval for a command.
    #[allow(dead_code)]
    pub fn approve_sudo(&self, command: &str, duration_secs: u64) {
        if let Ok(mut sessions) = self.sudo_sessions.lock() {
            let expiry = std::time::Instant::now() + std::time::Duration::from_secs(duration_secs);
            sessions.insert(command.to_string(), expiry);
            info!(
                "Sudo approved for '{}' for {} seconds",
                command, duration_secs
            );
        }
    }

    /// Revoke all sudo approvals.
    #[allow(dead_code)]
    pub fn revoke_sudo(&self) {
        if let Ok(mut sessions) = self.sudo_sessions.lock() {
            sessions.clear();
            info!("All sudo approvals revoked");
        }
    }
}

/// Redact common secret patterns from text.
pub fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();

    // API keys
    let patterns = [
        (r"sk-[a-zA-Z0-9]{48}", "sk-***"),
        (r"ghp_[a-zA-Z0-9]{36}", "ghp_***"),
        (r"gho_[a-zA-Z0-9]{36}", "gho_***"),
        (r"AKIA[0-9A-Z]{16}", "AKIA***"),
        (r"\b[a-zA-Z0-9+/=]{80,}\b", "***"), // Generic long base64-like token
    ];

    for (pattern, replacement) in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            result = re.replace_all(&result, *replacement).to_string();
        }
    }

    // Redact env vars that look like secrets
    if let Ok(re) = regex::Regex::new(r"(?i)(API_KEY|SECRET|TOKEN|PWD|PASSWORD)=([^\s]+)") {
        result = re.replace_all(&result, "${1}=***").to_string();
    }

    result
}

/// Check if a path is within the allowed working directory.
#[allow(dead_code)]
pub fn is_path_allowed(path: &Path, working_dir: Option<&Path>) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    if let Some(wd) = working_dir {
        let wd = wd.canonicalize().unwrap_or_else(|_| wd.to_path_buf());
        path.starts_with(&wd)
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_config_default() {
        let config = SecurityConfig::default();
        assert_eq!(config.version, crate::VERSION);
        assert!(config.pii_redaction_enabled);
        assert!(config.prompt_injection_detection_enabled);
    }

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::None < RiskLevel::Low);
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn test_redact_secrets() {
        let text = "API_KEY=sk-abc123ghp_token123AKIA1234567890ABCDEF";
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("sk-abc123"));
        assert!(!redacted.contains("ghp_token123"));
        assert!(redacted.contains("API_KEY=***") || redacted.contains("=***"));
    }

    #[test]
    fn test_is_path_allowed() {
        let wd = Path::new("/home/user/project");
        assert!(is_path_allowed(
            Path::new("/home/user/project/src"),
            Some(wd)
        ));
        assert!(!is_path_allowed(Path::new("/etc/passwd"), Some(wd)));
        assert!(is_path_allowed(Path::new("/any/path"), None));
    }

    #[test]
    fn test_security_decision_variants() {
        let allow = SecurityDecision::Allow;
        let deny = SecurityDecision::Deny {
            reason: "test".to_string(),
        };
        let approval = SecurityDecision::RequireApproval {
            reason: "test".to_string(),
            risk_level: RiskLevel::Medium,
        };

        match allow {
            SecurityDecision::Allow => {}
            _ => panic!("Expected Allow"),
        }
        match deny {
            SecurityDecision::Deny { reason } => assert_eq!(reason, "test"),
            _ => panic!("Expected Deny"),
        }
        match approval {
            SecurityDecision::RequireApproval { risk_level, .. } => {
                assert_eq!(risk_level, RiskLevel::Medium)
            }
            _ => panic!("Expected RequireApproval"),
        }
    }
}
