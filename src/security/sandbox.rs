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

/// Canonicalize a path that may not exist yet: resolve the longest existing
/// ancestor (following symlinks such as macOS `/var` -> `/private/var` or
/// `/tmp` -> `/private/tmp`) and re-append the missing tail.
fn canonicalize_lenient(path: &Path) -> PathBuf {
    // `~` is expanded and `..` / `.` are resolved lexically first, so a path
    // through a missing directory ("<wd>/missing/../../etc/passwd") cannot
    // keep the allowed prefix when its tail is re-attached below.
    let path = normalize_lexical(path);
    let path = path.as_path();
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    let mut tail = Vec::new();
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(mut c) = parent.canonicalize() {
            for part in tail.iter().rev() {
                c.push(part);
            }
            return c;
        }
        cur = parent;
    }
    path.to_path_buf()
}

/// Expand `~`, make the path absolute (against the process cwd) and resolve
/// `.` and `..` components. Existing prefixes are canonicalized as the walk
/// goes, so `..` after a symlink follows the real parent; missing components
/// are resolved lexically. `..` above the root stays at the root.
fn normalize_lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let expanded = PathBuf::from(shellexpand::tilde(&path.to_string_lossy()).to_string());
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(expanded)
    };
    let mut out = PathBuf::new();
    for comp in absolute.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Normal(name) => {
                out.push(name);
                if let Ok(real) = out.canonicalize() {
                    out = real;
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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

        let canonical = canonicalize_lenient(path);
        for allowed in &self.allowed_paths {
            let allowed_canonical = canonicalize_lenient(allowed);
            if canonical.starts_with(&allowed_canonical) {
                return true;
            }
        }
        false
    }

    /// The system temp dir is always writable scratch space (OpenCode
    /// whitelists its tmp dir the same way).
    fn is_under_tmp(&self, path: &Path) -> bool {
        let tmp_canonical = canonicalize_lenient(&std::env::temp_dir());
        canonicalize_lenient(path).starts_with(&tmp_canonical)
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
            let canonical = canonicalize_lenient(&path);
            if self.is_allowed_internal(&canonical) {
                continue;
            }
            let glob = Self::dir_glob(&canonical);
            if let Ok(mut approved) = self.session_approved.lock()
                && !approved.contains(&glob)
            {
                info!("Session-approved external path: {}", glob);
                approved.push(glob);
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
            let wd_canonical = canonicalize_lenient(wd);
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
            let canonical = canonicalize_lenient(&path);
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
    #[allow(dead_code)]
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
/// Handles plain-text args ("read /home/user/file.txt"), JSON-formatted args
/// (pretty or compact, e.g. `{"path":"/etc/passwd"}`, nested values and
/// several concatenated objects) and relative traversal (`../x`, `..`).
fn extract_paths_from_args(args: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // JSON first: walk every string value (keys are ignored).
    let mut stream = serde_json::Deserializer::from_str(args).into_iter::<serde_json::Value>();
    let mut parsed_any = false;
    let mut json_ok = true;
    for value in stream.by_ref() {
        match value {
            Ok(v) => {
                parsed_any = true;
                collect_json_paths(&v, &mut paths);
            }
            Err(_) => {
                json_ok = false;
                break;
            }
        }
    }
    let is_json = parsed_any && json_ok && args.trim_start().starts_with(['{', '[']);

    if !is_json {
        extract_paths_from_text(args, &mut paths);
        // JSON-ish text that did not parse (e.g. a trailing comma): look at
        // every quoted string as well.
        for quoted in quoted_strings(args) {
            extract_paths_from_text(&quoted, &mut paths);
        }
    }

    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    paths
}

fn collect_json_paths(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => {
            if !s.contains(char::is_whitespace) {
                if looks_like_path(s) {
                    out.push(s.clone());
                }
                return;
            }
            // A path with spaces ("/home/u/My Docs/a.txt") counts as a whole...
            if s.starts_with(['/', '~'])
                || s.starts_with("./")
                || s.starts_with("../")
                || Path::new(s).is_absolute()
            {
                out.push(s.clone());
            }
            // ...and so does each path-like word, e.g. a command line inside a
            // value ("cat ../../etc/passwd").
            extract_paths_from_text(s, out);
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_json_paths(v, out)),
        serde_json::Value::Object(map) => map.values().for_each(|v| collect_json_paths(v, out)),
        _ => {}
    }
}

fn looks_like_path(token: &str) -> bool {
    if token.starts_with("http://") || token.starts_with("https://") {
        return false;
    }
    token.starts_with('/')
        || token.starts_with('~')
        || token.starts_with("./")
        || token.starts_with("../")
        || token == ".."
        || token.contains("/../")
        || token.ends_with("/..")
        // Windows: `C:\x`, `\\server\share`, `..\x`, `a\..\b`
        || Path::new(token).is_absolute()
        || token.starts_with("..\\")
        || token.contains("\\..\\")
        || token.contains("\\../")
        || token.contains("/..\\")
}

fn extract_paths_from_text(args: &str, out: &mut Vec<String>) {
    for token in args.split_whitespace() {
        let token = token.trim_matches('"').trim_matches('\'');

        // `--file=/etc/passwd` / `path=../x`: judge the value.
        let token = match token.split_once('=') {
            Some((_, value)) if token.starts_with('-') || !token.starts_with('/') => {
                value.trim_matches('"').trim_matches('\'')
            }
            _ => token,
        };

        // Skip flags and options
        if token.starts_with('-') {
            continue;
        }

        if looks_like_path(token) {
            // Strip trailing JSON delimiters that may leak in from JSON-formatted args.
            // e.g. "/home/synth"}" → "/home/synth"
            let cleaned = token.trim_end_matches(|c: char| {
                c == '}' || c == ']' || c == ',' || c == '"' || c == '\''
            });
            if !cleaned.is_empty() {
                out.push(cleaned.to_string());
            }
        }
    }
}

/// Contents of every double-quoted string (with `\"` escapes) in `text`.
fn quoted_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut cur = String::new();
        let mut closed = false;
        while let Some(n) = chars.next() {
            match n {
                '\\' => {
                    if let Some(e) = chars.next() {
                        cur.push(e);
                    }
                }
                '"' => {
                    closed = true;
                    break;
                }
                other => cur.push(other),
            }
        }
        if closed {
            out.push(cur);
        }
    }
    out
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
    fn test_extract_paths_compact_and_nested_json() {
        assert_eq!(
            extract_paths_from_args("{\"path\":\"/etc/passwd\"}"),
            vec!["/etc/passwd"]
        );
        assert_eq!(
            extract_paths_from_args(
                "{\"op\":\"copy\",\"args\":{\"from\":\"/etc/shadow\",\"to\":[\"../out\"]}}"
            ),
            vec!["/etc/shadow", "../out"]
        );
        // a command line inside a JSON value
        assert_eq!(
            extract_paths_from_args("{\"command\":\"cat ../../etc/passwd\"}"),
            vec!["../../etc/passwd"]
        );
        // a path with spaces is checked whole (and word by word)
        assert_eq!(
            extract_paths_from_args("{\"path\":\"/home/u/My Docs/a.txt\"}"),
            vec!["/home/u/My Docs/a.txt", "/home/u/My"]
        );
        // keys never count as paths
        assert!(extract_paths_from_args("{\"/etc/passwd\":\"x\"}").is_empty());
    }

    #[test]
    fn test_extract_paths_traversal() {
        assert_eq!(
            extract_paths_from_args("read ../secret.txt"),
            vec!["../secret.txt"]
        );
        assert_eq!(extract_paths_from_args("cd .."), vec![".."]);
        assert_eq!(
            extract_paths_from_args("cat foo/../../etc/passwd"),
            vec!["foo/../../etc/passwd"]
        );
        assert_eq!(
            extract_paths_from_args("grep x --file=/etc/passwd"),
            vec!["/etc/passwd"]
        );
        assert!(extract_paths_from_args("fetch https://example.com/a/../b").is_empty());
    }

    #[test]
    fn test_normalize_lexical_resolves_dotdot() {
        let base = std::env::temp_dir().canonicalize().unwrap();
        let missing = format!("os-norm-{}", uuid::Uuid::new_v4().simple());
        assert_eq!(
            normalize_lexical(
                &base
                    .join(&missing)
                    .join("a")
                    .join("..")
                    .join("..")
                    .join("y")
            ),
            base.join("y")
        );
        assert_eq!(
            normalize_lexical(&base.join(&missing).join(".").join("b")),
            base.join(&missing).join("b")
        );
        #[cfg(unix)]
        assert_eq!(
            normalize_lexical(Path::new(&format!("/../../{missing}"))),
            PathBuf::from(format!("/{missing}"))
        );
    }

    #[test]
    fn test_sandbox_blocks_dotdot_through_missing_dir() {
        let wd = std::env::temp_dir().join(format!("os-sbx-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&wd).unwrap();
        let mut sandbox = Sandbox::new(Some(wd.clone())).unwrap();
        sandbox.set_external_mode(ExternalMode::Deny);
        let up = "../".repeat(12);
        let escape_path = format!("{}/missing/{up}etc/passwd", wd.display());
        assert!(
            sandbox
                .validate_path("fs", &format!("read {escape_path}"))
                .is_err(),
            "traversal through a missing dir"
        );
        let escape_json = serde_json::json!({ "path": escape_path }).to_string();
        assert!(
            sandbox.validate_path("fs", &escape_json).is_err(),
            "compact JSON traversal"
        );
        let outside = if cfg!(windows) {
            "C:\\Windows\\win.ini"
        } else {
            "/etc/passwd"
        };
        let outside_json = serde_json::json!({ "path": outside }).to_string();
        assert!(
            sandbox.validate_path("fs", &outside_json).is_err(),
            "compact JSON"
        );
        let inside =
            serde_json::json!({ "path": format!("{}/sub/../file.txt", wd.display()) }).to_string();
        assert!(
            sandbox.validate_path("fs", &inside).is_ok(),
            "dotdot that stays inside"
        );
        let _ = std::fs::remove_dir_all(&wd);
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
