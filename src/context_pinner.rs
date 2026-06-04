//! Smart Context — Manually pin files to the conversation context.
//!
//! Users can pin specific files with `/ctx <path>` and they'll be
//! included in every system prompt until cleared with `/ctx clear`.
//!
//! Persisted per-session to `~/.config/openshark/context/<session_id>.json`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A pinned file entry with optional metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedFile {
    pub path: String,
    pub added_at: chrono::DateTime<chrono::Utc>,
    /// Optional note about why this file is pinned.
    pub note: Option<String>,
}

/// Smart context state — pinned files for a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SmartContext {
    pub session_id: String,
    pub pinned: Vec<PinnedFile>,
}

impl SmartContext {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            pinned: Vec::new(),
        }
    }

    /// Load smart context for a session from disk.
    pub fn load(session_id: &str) -> Self {
        let path = Self::storage_path(session_id);
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(ctx) = serde_json::from_str::<SmartContext>(&data) {
                return ctx;
            }
        }
        Self::new(session_id)
    }

    /// Save smart context to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::storage_path(&self.session_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    /// Pin a file to context.
    pub fn pin(&mut self, path: &str, note: Option<String>) -> Result<String> {
        let canonical = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string());

        // Deduplicate
        if self.pinned.iter().any(|f| f.path == canonical) {
            return Ok(format!("📌 Already pinned: {}", canonical));
        }

        self.pinned.push(PinnedFile {
            path: canonical.clone(),
            added_at: chrono::Utc::now(),
            note,
        });
        self.save()?;
        Ok(format!("📌 Pinned: {}", canonical))
    }

    /// Unpin a specific file.
    pub fn unpin(&mut self, path: &str) -> Result<String> {
        let before = self.pinned.len();
        self.pinned.retain(|f| !f.path.ends_with(path) && f.path != path);
        let removed = before - self.pinned.len();
        self.save()?;
        if removed > 0 {
            Ok(format!("📍 Unpinned: {}", path))
        } else {
            Ok(format!("📍 Not found in pinned files: {}", path))
        }
    }

    /// Clear all pinned files.
    pub fn clear(&mut self) -> Result<String> {
        let count = self.pinned.len();
        self.pinned.clear();
        self.save()?;
        Ok(format!("📍 Cleared {} pinned file(s)", count))
    }

    /// List pinned files as formatted lines.
    pub fn list(&self) -> Vec<String> {
        if self.pinned.is_empty() {
            return vec!["📍 No pinned files. Use /ctx <path> to pin.".to_string()];
        }
        let mut lines = vec![format!("📍 Pinned Files ({}):", self.pinned.len())];
        for (i, file) in self.pinned.iter().enumerate() {
            let note_str = file.note.as_ref().map(|n| format!(" — {}", n)).unwrap_or_default();
            lines.push(format!("  {}. {}{}", i + 1, file.path, note_str));
        }
        lines
    }

    /// Format pinned files into a context block for the system prompt.
    pub fn format_context_block(&self) -> String {
        if self.pinned.is_empty() {
            return String::new();
        }

        let mut lines = vec![
            "\n📌 PINNED CONTEXT FILES:\n".to_string(),
            "─".repeat(50),
        ];

        for file in &self.pinned {
            if let Ok(content) = std::fs::read_to_string(&file.path) {
                let lang = detect_lang(&file.path);
                let snippet: String = content.lines().take(30).collect::<Vec<_>>().join("\n");
                lines.push(format!("\n  📄 {}\n  ```{}\n{}\n  ```", file.path, lang, snippet));
            } else {
                lines.push(format!("\n  📄 {} (unreadable)", file.path));
            }
        }

        lines.push(String::new());
        lines.join("\n")
    }

    fn storage_path(session_id: &str) -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("openshark")
            .join("context")
            .join(format!("{}.json", session_id))
    }
}

fn detect_lang(path: &str) -> &'static str {
    match std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("py") => "python",
        Some("js") => "javascript",
        Some("ts") => "typescript",
        Some("go") => "go",
        Some("c") => "c",
        Some("cpp") | Some("cc") | Some("hpp") => "cpp",
        Some("java") => "java",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("swift") => "swift",
        Some("kt") => "kotlin",
        Some("sh") => "bash",
        Some("yaml") | Some("yml") => "yaml",
        Some("json") => "json",
        Some("toml") => "toml",
        Some("md") => "markdown",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_and_clear() {
        let mut ctx = SmartContext::new("test-session");
        assert!(ctx.pinned.is_empty());

        let result = ctx.pin("/tmp/test_file.rs", Some("main entry".to_string())).unwrap();
        assert!(result.contains("Pinned"));
        assert_eq!(ctx.pinned.len(), 1);

        let result = ctx.clear().unwrap();
        assert!(result.contains("Cleared"));
        assert!(ctx.pinned.is_empty());
    }

    #[test]
    fn test_deduplicate() {
        let mut ctx = SmartContext::new("test-session");
        ctx.pin("/tmp/test.rs", None).unwrap();
        let result = ctx.pin("/tmp/test.rs", None).unwrap();
        assert!(result.contains("Already pinned"));
        assert_eq!(ctx.pinned.len(), 1);
    }

    #[test]
    fn test_list_empty() {
        let ctx = SmartContext::new("test-session");
        let lines = ctx.list();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("No pinned files"));
    }

    #[test]
    fn test_format_context_block_empty() {
        let ctx = SmartContext::new("test-session");
        assert!(ctx.format_context_block().is_empty());
    }
}
