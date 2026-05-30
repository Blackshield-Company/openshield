//! Sandbox / Infrastructure Isolation
//!
//! Provides working directory isolation and path validation.
//! The sandbox ensures tool execution is constrained to an allowed directory
//! unless explicitly configured otherwise.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

/// Manages working directory isolation for tool execution.
pub struct Sandbox {
    /// The enforced working directory. None = no restriction.
    working_dir: Option<PathBuf>,
    /// Whether to allow commands to escape the working directory.
    allow_escape: bool,
}

impl Sandbox {
    pub fn new(working_dir: Option<PathBuf>) -> Result<Self> {
        let wd = working_dir.map(|p| {
            let expanded = shellexpand::tilde(&p.to_string_lossy()).to_string();
            PathBuf::from(expanded)
        });

        if let Some(ref dir) = wd {
            if !dir.exists() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("Failed to create sandbox directory: {}", dir.display()))?;
                info!("Created sandbox directory: {}", dir.display());
            }
        }

        Ok(Self {
            working_dir: wd,
            allow_escape: false,
        })
    }

    #[allow(dead_code)]
    pub fn new_with_escape(working_dir: Option<PathBuf>, allow_escape: bool) -> Result<Self> {
        let mut sandbox = Self::new(working_dir)?;
        sandbox.allow_escape = allow_escape;
        Ok(sandbox)
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

    /// Validate that a tool's arguments stay within the sandbox.
    pub fn validate_path(&self, tool_name: &str, args: &str) -> Result<(), String> {
        // Tools that don't access filesystem are always allowed
        let fs_tools = ["fs", "terminal", "edit", "search", "git"];
        if !fs_tools.contains(&tool_name) {
            return Ok(());
        }

        let wd = match self.working_dir {
            Some(ref dir) => dir,
            None => return Ok(()), // No sandbox = no restriction
        };

        if self.allow_escape {
            return Ok(());
        }

        // Extract paths from arguments
        let paths = extract_paths_from_args(args);
        for path_str in paths {
            let path = PathBuf::from(&path_str);
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            let wd_canonical = wd.canonicalize().unwrap_or_else(|_| wd.clone());

            if !canonical.starts_with(&wd_canonical) {
                return Err(format!(
                    "Path '{}' is outside the allowed working directory '{}'. \
                     Use --allow-escape or change working directory.",
                    path_str,
                    wd.display()
                ));
            }
        }

        Ok(())
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
        if token.starts_with('/') || token.starts_with("~") || token.starts_with("./") {
            paths.push(token.to_string());
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
        let sandbox = Sandbox::new(Some(tmp.clone())).unwrap();

        // Outside sandbox should fail
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
    fn test_extract_paths() {
        let paths = extract_paths_from_args("read /home/user/file.txt --flag value");
        assert_eq!(paths, vec!["/home/user/file.txt"]);

        let paths = extract_paths_from_args("read ./file.txt ~/other.txt");
        assert_eq!(paths, vec!["./file.txt", "~/other.txt"]);

        let paths = extract_paths_from_args("echo hello");
        assert!(paths.is_empty());
    }
}
