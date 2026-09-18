//! Sandbox / Infrastructure Isolation
//!
//! Provides working directory isolation and path validation.
//!
//! Permission model is OpenCode-parity (anomalyco/opencode v2,
//! packages/opencode/src/permission + tool/external-directory.ts):
//!
//!  - Access INSIDE the working directory (and configured allowed paths)
//!    is always allowed.
//!  - The system temp dir is always allowed (opencode whitelists its tmp
//!    dir the same way) — scratch files must never bounce.
//!  - Access OUTSIDE is **ask** by default: the call routes to the human
//!    approval gate (M3 approval_req) instead of being refused outright.
//!    Approving records the directory glob for the rest of the session
//!    (opencode's "always" reply), so repeat access flows.
//!  - `external_access = "deny"` restores the legacy hard jail;
//!    `"allow"` (or --allow-escape) disables external checks.
//!
//! Refusals and asks always say where the model CAN write — a bare denial
//! sends agents flailing through /tmp and $HOME guesses.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::info;

/// External-access policy for paths outside the working directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalMode {
    /// Route to the human approval gate (default, OpenCode parity).
    Ask,
    /// Hard jail — refuse outright (legacy behavior).
    Deny,
    /// No external restriction.
    Allow,
}

impl ExternalMode {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "deny" | "jail" | "strict" => ExternalMode::Deny,
            "allow" | "off" | "none" => ExternalMode::Allow,
            _ => ExternalMode::Ask,
        }
    }
}

/// Outcome of evaluating a tool call against the sandbox.
#[derive(Clone, Debug, PartialEq)]
pub enum SandboxVerdict {
    Allow,
    /// Needs a human decision; includes the session-approvable glob.
    Ask {
        pattern: String,
        reason: String,
    },
    Deny {
        reason: String,
    },
}

/// Manages working directory isolation for tool execution.
#[derive(Clone)]
pub struct Sandbox {
    /// The enforced working directory. None = no restriction.
    working_dir: Option<PathBuf>,
    /// Whether to allow commands to escape the working directory.
    allow_escape: bool,
    /// User-configured allowed paths. Empty = no restriction.
    allowed_paths: Vec<PathBuf>,
    /// Policy for paths outside the allowed set.
    external_mode: ExternalMode,
    /// Directory globs approved by the human this session ("always").
    session_approved: Arc<Mutex<Vec<String>>>,
}

impl Sandbox {
    pub fn new(working_dir: Option<PathBuf>) -> Result<Self> {
        let wd = working_dir.map(|p| {
            let expanded = shellexpand::tilde(&p.to_string_lossy()).to_string();
            PathBuf::from(expanded)
        });

        if let Some(ref dir) = wd
            && !dir.exists()
        {
            std::fs::create_dir_all(dir).with_context(|| {
                format!("Failed to create sandbox directory: {}", dir.display())
            })?;
            info!("Created sandbox directory: {}", dir.display());
        }

        Ok(Self {
            working_dir: wd,
            allow_escape: false,
            allowed_paths: Vec::new(),
            external_mode: ExternalMode::Ask,
            session_approved: Arc::new(Mutex::new(Vec::new())),
        })
    }

    #[allow(dead_code)]
    pub fn new_with_escape(working_dir: Option<PathBuf>, allow_escape: bool) -> Result<Self> {
        let mut sandbox = Self::new(working_dir)?;
        sandbox.allow_escape = allow_escape;
        if allow_escape {
            sandbox.external_mode = ExternalMode::Allow;
        }
        Ok(sandbox)
    }

    /// Set user-configured allowed paths (from FilesystemConfig).
    pub fn set_allowed_paths(&mut self, paths: Vec<String>) {
        self.allowed_paths = paths
            .into_iter()
            .map(|p| {
                let expanded = shellexpand::tilde(&p).to_string();
                PathBuf::from(expanded)
            })
            .collect();
        info!("Sandbox allowed paths updated: {:?}", self.allowed_paths);
    }

    /// Set the external-access policy (from SecurityConfig.external_access).
    pub fn set_external_mode(&mut self, mode: ExternalMode) {
        self.external_mode = mode;
        info!("Sandbox external access mode: {:?}", mode);
    }

    /// Get the current working directory restriction.
    #[allow(dead_code)]
    pub fn working_dir(&self) -> Option<&Path> {
        self.working_dir.as_deref()
    }

    /// Check if escape is allowed.
    #[allow(dead_code)]
    pub fn allows_escape(&self) -> bool {
        self.allow_escape
    }

    /// Check if a path is within any of the allowed paths.
    fn is_in_allowed_paths(&self, path: &Path) -> bool {
        if self.allowed_paths.is_empty() {
            return true; // No restriction
        }

        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        for allowed in &self.allowed_paths {
            let allowed_canonical = allowed.canonicalize().unwrap_or_else(|_| allowed.clone());
            if canonical.starts_with(&allowed_canonical) {
                return true;
            }
        }
        false
    }

    /// The system temp dir is always writable scratch space (OpenCode
    /// whitelists its tmp dir the same way).
    fn is_under_tmp(&self, path: &Path) -> bool {
        let tmp = std::env::temp_dir();
        let tmp_canonical = tmp.canonicalize().unwrap_or(tmp);
        path.starts_with(&tmp_canonical)
    }

    /// Glob-style check against session-approved directory patterns.
    /// Patterns are "<dir>/*" — plain prefix match on the directory.
    fn is_session_approved(&self, path: &Path) -> bool {
        let approved = self
            .session_approved
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        approved.iter().any(|pattern| {
            let prefix = pattern.trim_end_matches('*').trim_end_matches('/');
            !prefix.is_empty() && path.starts_with(prefix)
        })
    }

    /// Directory glob for a target path, opencode-style: files approve their
    /// parent dir ("<dir>/*"), directories approve themselves.
    fn dir_glob(path: &Path) -> String {
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| path.to_path_buf())
        };
        format!("{}/*", dir.display())
    }

    /// Record session approval for the external paths in this tool call
    /// (invoked after the human approves at the gate — opencode "always").
    pub fn record_session_approval(&self, tool_name: &str, args: &str) {
        let fs_tools = ["fs", "terminal", "edit", "search", "git"];
        if !fs_tools.contains(&tool_name) {
            return;
        }
        for path_str in extract_paths_from_args(args) {
            let path = PathBuf::from(&path_str);
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if self.is_allowed_internal(&canonical) {
                continue;
            }
            let glob = Self::dir_glob(&canonical);
            if let Ok(mut approved) = self.session_approved.lock() {
                if !approved.contains(&glob) {
                    info!("Session-approved external path: {}", glob);
                    approved.push(glob);
                }
            }
        }
    }

    /// Inside the always-allowed set: working dir, allowed paths, tmp,
    /// or a session-approved glob.
    fn is_allowed_internal(&self, canonical: &Path) -> bool {
        if !self.allowed_paths.is_empty() {
            return self.is_in_allowed_paths(canonical)
                || self.is_under_tmp(canonical)
                || self.is_session_approved(canonical);
        }
        if let Some(ref wd) = self.working_dir {
            let wd_canonical = wd.canonicalize().unwrap_or_else(|_| wd.clone());
            if canonical.starts_with(&wd_canonical) {
                return true;
            }
        }
        self.is_under_tmp(canonical) || self.is_session_approved(canonical)
    }

    /// OpenCode-parity evaluation: Allow inside, Ask outside (default),
    /// Deny only in strict mode.
    pub fn evaluate(&self, tool_name: &str, args: &str) -> SandboxVerdict {
        // Tools that don't access filesystem are always allowed
        let fs_tools = ["fs", "terminal", "edit", "search", "git"];
        if !fs_tools.contains(&tool_name) {
            return SandboxVerdict::Allow;
        }

        if self.allow_escape || self.external_mode == ExternalMode::Allow {
            return SandboxVerdict::Allow;
        }

        // No restriction configured at all
        if self.allowed_paths.is_empty() && self.working_dir.is_none() {
            return SandboxVerdict::Allow;
        }

        let allowed_summary = if !self.allowed_paths.is_empty() {
            self.allowed_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            self.working_dir
                .as_ref()
                .map(|d| d.display().to_string())
                .unwrap_or_default()
        };

        for path_str in extract_paths_from_args(args) {
            let path = PathBuf::from(&path_str);
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if self.is_allowed_internal(&canonical) {
                continue;
            }
            let glob = Self::dir_glob(&canonical);
            match self.external_mode {
                ExternalMode::Allow => return SandboxVerdict::Allow,
                ExternalMode::Deny => {
                    return SandboxVerdict::Deny {
                        reason: format!(
                            "Path '{}' is outside the allowed directories ({}). \
                             Retry with a path inside '{}' (e.g. a scratch/demo folder there). \
                             To permit outside paths: set external_access = \"ask\" or use --allow-escape.",
                            path_str, allowed_summary, allowed_summary
                        ),
                    };
                }
                ExternalMode::Ask => {
                    return SandboxVerdict::Ask {
                        pattern: glob.clone(),
                        reason: format!(
                            "external_directory: '{}' is outside the working area ({}). \
                             Approve to allow '{}' for this session. \
                             Prefer a path inside '{}' when possible.",
                            path_str, allowed_summary, glob, allowed_summary
                        ),
                    };
                }
            }
        }

        SandboxVerdict::Allow
    }

    /// Validate that a tool's arguments stay within the sandbox.
    /// Legacy hard-gate wrapper around evaluate(): Ask and Deny both
    /// surface as Err; prefer evaluate() for the OpenCode-parity flow.
    pub fn validate_path(&self, tool_name: &str, args: &str) -> Result<(), String> {
        match self.evaluate(tool_name, args) {
            SandboxVerdict::Allow => Ok(()),
            SandboxVerdict::Ask { reason, .. } | SandboxVerdict::Deny { reason } => Err(reason),
        }
    }

    /// Change the working directory at runtime.
    #[allow(dead_code)]
    pub fn set_working_dir(&mut self, path: PathBuf) -> Result<()> {
        let expanded = shellexpand::tilde(&path.to_string_lossy()).to_string();
        let path = PathBuf::from(expanded);

        if !path.exists() {
            std::fs::create_dir_all(&path)
                .with_context(|| format!("Failed to create directory: {}", path.display()))?;
        }

        self.working_dir = Some(path);
        info!("Sandbox working directory updated");
        Ok(())
    }

    /// Clear the working directory restriction.
    #[allow(dead_code)]
    pub fn clear_working_dir(&mut self) {
        self.working_dir = None;
        info!("Sandbox working directory restriction cleared");
    }
}

/// Extract potential file paths from tool arguments.
/// Handles both plain-text args ("read /home/user/file.txt") and
/// JSON-formatted args ("{\"operation\": \"read\", \"path\": \"/home/user/file.txt\"}").
fn extract_paths_from_args(args: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Split by common delimiters and check each token
    for token in args.split_whitespace() {
        let token = token.trim_matches('"').trim_matches('\'');

        // Skip flags and options
        if token.starts_with('-') || token.starts_with("--") {
            continue;
        }

        // Skip URLs
        if token.starts_with("http://") || token.starts_with("https://") {
            continue;
        }

        // Check if it looks like a path
        if token.starts_with('/') || token.starts_with('~') || token.starts_with("./") {
            // Strip trailing JSON delimiters that may leak in from JSON-formatted args.
            // e.g. "/home/synth"}" → "/home/synth"
            let cleaned = token.trim_end_matches(|c: char| {
                c == '}' || c == ']' || c == ',' || c == '"' || c == '\''
            });
            paths.push(cleaned.to_string());
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_no_restriction() {
        let sandbox = Sandbox::new(None).unwrap();
        assert!(sandbox.working_dir().is_none());
        assert!(sandbox.validate_path("fs", "read /etc/passwd").is_ok());
    }

    #[test]
    fn test_sandbox_with_restriction() {
        let tmp = std::env::temp_dir();
        let sandbox = Sandbox::new(Some(tmp.clone())).unwrap();
        assert!(sandbox.working_dir().is_some());

        // Within sandbox should succeed
        let subdir = tmp.join("subdir");
        let result = sandbox.validate_path("fs", &format!("read {}", subdir.display()));
        assert!(result.is_ok());
    }

    #[test]
    fn test_sandbox_escape_blocked() {
        let tmp = std::env::temp_dir();
        let mut sandbox = Sandbox::new(Some(tmp.clone())).unwrap();
        sandbox.set_external_mode(ExternalMode::Deny);

        // Outside sandbox should fail in strict (legacy jail) mode
        let result = sandbox.validate_path("fs", "read /etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_sandbox_escape_allowed() {
        let tmp = std::env::temp_dir();
        let sandbox = Sandbox::new_with_escape(Some(tmp.clone()), true).unwrap();

        // Outside sandbox should succeed when escape is allowed
        let result = sandbox.validate_path("fs", "read /etc/passwd");
        assert!(result.is_ok());
    }

    #[test]
    fn test_sandbox_allowed_paths() {
        let mut sandbox = Sandbox::new(None).unwrap();
        sandbox.set_allowed_paths(vec!["/tmp".to_string()]);
        sandbox.set_external_mode(ExternalMode::Deny);

        // Within allowed path should succeed
        assert!(sandbox.validate_path("fs", "read /tmp/test.txt").is_ok());

        // Outside allowed path should fail in strict mode
        let result = sandbox.validate_path("fs", "read /etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_sandbox_allowed_paths_home() {
        let mut sandbox = Sandbox::new(None).unwrap();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        sandbox.set_allowed_paths(vec![home.to_string_lossy().to_string()]);

        // Home path should succeed
        let test_path = home.join(".config/test");
        assert!(
            sandbox
                .validate_path("fs", &format!("read {}", test_path.display()))
                .is_ok()
        );
    }

    #[test]
    fn test_external_default_is_ask_not_deny() {
        // OpenCode parity: outside paths must route to the approval gate,
        // not bounce with a hard refusal.
        let tmp = std::env::temp_dir();
        let sandbox = Sandbox::new(Some(tmp.clone())).unwrap();
        match sandbox.evaluate("fs", "read /etc/passwd") {
            SandboxVerdict::Ask { pattern, reason } => {
                assert!(pattern.ends_with("/*"));
                assert!(reason.contains("external_directory"));
            }
            other => panic!("expected Ask, got {:?}", other),
        }
    }

    #[test]
    fn test_tmp_dir_always_allowed() {
        // OpenCode whitelists its tmp dir — scratch files must never bounce,
        // even when the working dir is elsewhere.
        let project = std::env::temp_dir().join("openshield_sandbox_project_test");
        std::fs::create_dir_all(&project).unwrap();
        let sandbox = Sandbox::new(Some(project.clone())).unwrap();
        let scratch = std::env::temp_dir().join("some_scratch_file.txt");
        assert_eq!(
            sandbox.evaluate("edit", &format!("write {} hello", scratch.display())),
            SandboxVerdict::Allow
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn test_session_approval_flows() {
        // After the human approves, repeat access to the same directory
        // auto-allows for the rest of the session (opencode "always").
        let project = std::env::temp_dir().join("openshield_sandbox_session_test");
        std::fs::create_dir_all(&project).unwrap();
        let sandbox = Sandbox::new(Some(project.clone())).unwrap();

        let external = "/var/log/openshield_demo.txt";
        assert!(matches!(
            sandbox.evaluate("fs", &format!("read {}", external)),
            SandboxVerdict::Ask { .. }
        ));

        sandbox.record_session_approval("fs", &format!("read {}", external));
        assert_eq!(
            sandbox.evaluate("fs", &format!("read {}", external)),
            SandboxVerdict::Allow
        );
        // Sibling file in the same approved directory flows too
        assert_eq!(
            sandbox.evaluate("fs", "read /var/log/other.txt"),
            SandboxVerdict::Allow
        );
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn test_deny_mode_message_says_where_to_write() {
        let tmp = std::env::temp_dir();
        let mut sandbox = Sandbox::new(Some(tmp.clone())).unwrap();
        sandbox.set_external_mode(ExternalMode::Deny);
        let result = sandbox.validate_path("fs", "read /etc/passwd");
        let msg = result.unwrap_err();
        assert!(msg.contains("Retry with a path inside"));
    }

    #[test]
    fn test_extract_paths() {
        let paths = extract_paths_from_args("read /home/user/file.txt --flag value");
        assert_eq!(paths, vec!["/home/user/file.txt"]);

        let paths = extract_paths_from_args("read ./file.txt ~/other.txt");
        assert_eq!(paths, vec!["./file.txt", "~/other.txt"]);

        let paths = extract_paths_from_args("echo hello");
        assert!(paths.is_empty());
    }

    #[test]
    fn test_extract_paths_from_json_args() {
        // Path with trailing JSON quote and brace — the bug this fixes
        let paths = extract_paths_from_args("{\"operation\": \"tree\", \"path\": \"/home/synth\"}");
        assert_eq!(paths, vec!["/home/synth"]);

        // Path with trailing quote (no brace)
        let paths = extract_paths_from_args("{\"operation\": \"read\", \"path\": \"/etc/hosts\"}");
        assert_eq!(paths, vec!["/etc/hosts"]);

        // Multiple paths in JSON
        let paths = extract_paths_from_args(
            "{\"operation\": \"read\", \"path\": \"/tmp/a.txt\"} {\"path\": \"/tmp/b.txt\"}",
        );
        assert_eq!(paths, vec!["/tmp/a.txt", "/tmp/b.txt"]);

        // JSON with trailing comma
        let paths = extract_paths_from_args("{\"operation\": \"list\", \"path\": \"/var/log\",}");
        assert_eq!(paths, vec!["/var/log"]);
    }

    #[test]
    fn test_extract_paths_plain_args_unaffected() {
        // Plain text args should still work correctly
        let paths = extract_paths_from_args("read /home/user/file.txt");
        assert_eq!(paths, vec!["/home/user/file.txt"]);

        let paths = extract_paths_from_args("tree /tmp 3");
        assert_eq!(paths, vec!["/tmp"]);

        let paths = extract_paths_from_args("cat /home/user/log.txt 0 100");
        assert_eq!(paths, vec!["/home/user/log.txt"]);
    }
}
