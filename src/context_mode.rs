//! Context Mode — Auto file identification for smarter agent context
//!
//! When enabled, OpenShark automatically identifies files relevant to the user's
//! query and injects them into the system prompt. This helps the model know
//! which files to read without the user explicitly mentioning them.
//!
//! Scoring factors:
//! - Filename matches query keywords
//! - Symbol names (functions, structs, etc.) match query keywords
//! - Files recently modified in git
//! - Files mentioned in recent conversation history

use anyhow::Result;
use std::collections::HashSet;

/// Configuration for context mode auto-identification.
#[derive(Debug, Clone)]
pub struct ContextModeConfig {
    /// Whether auto-context is enabled.
    pub enabled: bool,
    /// Maximum number of files to include in context.
    pub max_files: usize,
    /// Minimum relevance score for a file to be included (0.0-1.0).
    pub min_score: f32,
    /// Include file contents (first N lines) in addition to paths.
    pub include_snippets: bool,
    /// Number of lines to include per file snippet.
    pub snippet_lines: usize,
}

impl Default for ContextModeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_files: 8,
            min_score: 0.15,
            include_snippets: false,
            snippet_lines: 20,
        }
    }
}

/// A scored file relevance result.
#[derive(Debug, Clone)]
pub struct RelevantFile {
    pub path: String,
    pub score: f32,
    pub reasons: Vec<String>,
}

/// The context mode engine — identifies relevant files for a query.
pub struct ContextModeEngine {
    pub config: ContextModeConfig,
    project_path: String,
    /// Cached repo map for the project.
    cached_repo_map: Option<crate::repo_map::RepoMap>,
    /// Cached git status for quick recency scoring.
    cached_git_files: Option<Vec<String>>,
}

impl ContextModeEngine {
    pub fn new(project_path: String) -> Self {
        Self {
            config: ContextModeConfig::default(),
            project_path,
            cached_repo_map: None,
            cached_git_files: None,
        }
    }


    /// Refresh the cached repo map and git status.
    pub fn refresh_cache(&mut self) -> Result<()> {
        if std::path::Path::new(&self.project_path).exists() {
            self.cached_repo_map = crate::repo_map::build_repo_map(&self.project_path).ok();

            // Cache recently modified files from git
            self.cached_git_files = Self::get_recent_git_files(&self.project_path).ok();
        }
        Ok(())
    }

    /// Identify files relevant to the given user query.
    pub fn identify_relevant_files(&mut self, query: &str) -> Vec<RelevantFile> {
        if !self.config.enabled {
            return Vec::new();
        }

        if self.cached_repo_map.is_none() {
            let _ = self.refresh_cache();
        }

        let repo_map = match self.cached_repo_map.as_ref() {
            Some(m) => m,
            None => return Vec::new(),
        };

        let keywords = extract_keywords(query);
        if keywords.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<RelevantFile> = Vec::new();
        let git_files: HashSet<String> = self
            .cached_git_files
            .as_ref()
            .map(|v| v.iter().cloned().collect())
            .unwrap_or_default();

        for file in &repo_map.files {
            let mut score = 0.0f32;
            let mut reasons = Vec::new();

            let file_lower = file.path.to_lowercase();
            let filename = std::path::Path::new(&file.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            // Keyword matches in filename
            for kw in &keywords {
                if filename.to_lowercase().contains(kw) {
                    score += 0.4;
                    reasons.push(format!("filename matches '{}'", kw));
                }
                if file_lower.contains(kw) {
                    score += 0.2;
                    reasons.push(format!("path contains '{}'", kw));
                }
            }

            // Symbol matches in this file
            for sym in &repo_map.symbols {
                if sym.file == file.path {
                    for kw in &keywords {
                        if sym.name.to_lowercase().contains(kw) {
                            score += 0.35;
                            reasons.push(format!("{} '{}' matches '{}'", sym.kind, sym.name, kw));
                        }
                    }
                }
            }

            // Recent git modification boost
            if git_files.contains(&file.path) {
                score += 0.15;
                reasons.push("recently modified".to_string());
            }

            // Language relevance boost for common query patterns
            for kw in &keywords {
                match kw.as_str() {
                    "test" | "tests" | "testing" if file.language == "rust" && file.path.contains("test") => {
                        score += 0.1;
                        reasons.push("test file".to_string());
                    }
                    "config" | "configuration" if file.path.contains("config") || file.path.contains("toml") => {
                        score += 0.1;
                        reasons.push("config file".to_string());
                    }
                    _ => {}
                }
            }

            if score >= self.config.min_score {
                // Deduplicate reasons
                let mut unique_reasons = Vec::new();
                for r in reasons {
                    if !unique_reasons.contains(&r) {
                        unique_reasons.push(r);
                    }
                }
                scored.push(RelevantFile {
                    path: file.path.clone(),
                    score,
                    reasons: unique_reasons,
                });
            }
        }

        // Sort by score descending
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(self.config.max_files);

        scored
    }

    /// Format relevant files into a context block for the system prompt.
    pub fn format_context_block(&mut self, query: &str) -> String {
        let files = self.identify_relevant_files(query);
        if files.is_empty() {
            return String::new();
        }

        let mut lines = vec![
            "\n📁 RELEVANT FILES FOR YOUR QUERY:\n".to_string(),
            "─".repeat(50),
        ];

        for file in &files {
            lines.push(format!(
                "  • {} (score: {:.2}) — {}",
                file.path,
                file.score,
                file.reasons.join(", ")
            ));

            if self.config.include_snippets {
                if let Ok(content) = std::fs::read_to_string(
                    std::path::Path::new(&self.project_path).join(&file.path),
                ) {
                    let snippet: String = content
                        .lines()
                        .take(self.config.snippet_lines)
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !snippet.is_empty() {
                        lines.push(format!("    ```{}\n{}\n    ```", detect_lang(&file.path), snippet));
                    }
                }
            }
        }

        lines.push(String::new());
        lines.push(
            "💡 These files were automatically identified as relevant. \
             You can read them with: fs read <path>"
                .to_string(),
        );
        lines.join("\n")
    }

    /// Get recently modified files from git status.
    fn get_recent_git_files(project_path: &str) -> Result<Vec<String>> {
        let output = std::process::Command::new("git")
            .args(["status", "--short"])
            .current_dir(project_path)
            .output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut files = Vec::new();
        for line in stdout.lines() {
            // Parse " M src/main.rs" or "?? new_file.rs"
            if line.len() >= 3 {
                let file_part = line[3..].trim();
                if !file_part.is_empty() {
                    files.push(file_part.to_string());
                }
            }
        }
        Ok(files)
    }
}

/// Extract meaningful keywords from a user query.
fn extract_keywords(query: &str) -> Vec<String> {
    let stop_words: HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "must", "shall", "can", "need", "dare",
        "ought", "used", "to", "of", "in", "for", "on", "with", "at", "by",
        "from", "as", "into", "through", "during", "before", "after", "above",
        "below", "between", "under", "and", "but", "or", "yet", "so", "if",
        "because", "although", "though", "while", "where", "when", "that",
        "which", "who", "whom", "whose", "what", "this", "these", "those",
        "i", "you", "he", "she", "it", "we", "they", "me", "him", "her",
        "us", "them", "my", "your", "his", "its", "our", "their", "mine",
        "yours", "hers", "ours", "theirs", "myself", "yourself", "himself",
        "herself", "itself", "ourselves", "themselves", "let", "let's",
        "please", "help", "how", "what", "where", "why", "who", "show",
        "tell", "give", "make", "create", "add", "remove", "delete", "fix",
        "update", "change", "modify", "edit", "write", "read", "find",
        "search", "look", "get", "set", "run", "build", "compile", "test",
        "check", "see", "want", "need", "like", "know", "think", "use",
        "using", "used", "work", "working", "try", "trying", "tried",
        "want", "wanted", "would", "should", "could", "can", "will",
        "about", "around", "over", "off", "down", "out", "up", "here",
        "there", "now", "then", "today", "tomorrow", "yesterday", "just",
        "only", "also", "even", "well", "very", "too", "much", "many",
        "more", "most", "some", "any", "all", "each", "every", "both",
        "few", "little", "less", "least", "other", "another", "such",
        "no", "not", "never", "always", "sometimes", "often", "usually",
        "really", "actually", "probably", "maybe", "perhaps", "sure",
        "okay", "ok", "yes", "no", "right", "left", "next", "last",
        "first", "second", "third", "new", "old", "good", "bad", "best",
        "better", "worse", "worst", "big", "small", "large", "little",
        "long", "short", "high", "low", "great", "little", "own", "same",
        "different", "early", "late", "young", "old", "important",
        "possible", "able", "sure", "certain", "clear", "easy", "hard",
        "difficult", "free", "full", "empty", "open", "closed", "whole",
        "part", "half", "quarter", "double", "single", "multiple",
        "various", "several", "certain", "specific", "general", "common",
        "usual", "normal", "special", "particular", "certain", "exact",
        "precise", "accurate", "correct", "wrong", "false", "true",
        "real", "actual", "virtual", "digital", "physical", "manual",
        "automatic", "auto", "direct", "indirect", "main", "primary",
        "secondary", "tertiary", "final", "initial", "original", "copy",
        "version", "edition", "release", "update", "upgrade", "downgrade",
        "install", "uninstall", "setup", "configure", "configuration",
        "setting", "settings", "option", "options", "preference",
        "default", "custom", "personal", "private", "public", "shared",
        "local", "remote", "global", "universal", "internal", "external",
        "inner", "outer", "upper", "lower", "front", "back", "side",
        "top", "bottom", "center", "middle", "beginning", "end", "start",
        "finish", "stop", "go", "come", "leave", "enter", "exit",
        "open", "close", "save", "load", "import", "export", "backup",
        "restore", "reset", "refresh", "reload", "restart", "reboot",
        "shutdown", "power", "turn", "switch", "toggle", "enable",
        "disable", "activate", "deactivate", "start", "begin", "init",
        "initialize", "terminate", "kill", "destroy", "remove", "clear",
        "clean", "purge", "delete", "erase", "wipe", "format", "reformat",
        "convert", "transform", "translate", "parse", "serialize",
        "deserialize", "encode", "decode", "encrypt", "decrypt",
        "compress", "decompress", "zip", "unzip", "pack", "unpack",
        "merge", "split", "join", "separate", "combine", "mix", "blend",
        "match", "compare", "contrast", "diff", "patch", "apply", "revert",
        "undo", "redo", "repeat", "again", "once", "twice", "thrice",
        "time", "times", "now", "later", "soon", "eventually", "finally",
        "initially", "previously", "before", "afterward", "afterwards",
        "meanwhile", "during", "while", "until", "till", "since", "for",
        "ago", "ahead", "behind", "beyond", "beside", "near", "far",
        "away", "apart", "together", "alone", "along", "across", "through",
        "throughout", "within", "without", "inside", "outside", "upon",
        "onto", "toward", "towards", "against", "among", "amongst",
        "besides", "except", "excluding", "including", "regarding",
        "concerning", "respecting", "considering", "given", "assuming",
        "supposing", "depending", "according", "due", "owing", "thanks",
        "accordingly", "consequently", "therefore", "thus", "hence",
        "so", "then", "thereby", "whereby", "wherein", "whereupon",
        "whenever", "wherever", "whatever", "whichever", "however",
        "whatever", "whoever", "whomever", "whosoever", "whatsoever",
    ]
    .iter()
    .copied()
    .collect();

    query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|s| s.len() >= 2)
        .map(|s| s.to_lowercase())
        .filter(|s| !stop_words.contains(s.as_str()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

fn detect_lang(path: &str) -> &'static str {
    match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
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
    fn test_extract_keywords_basic() {
        let query = "How do I fix the authentication bug in login.rs?";
        let keywords = extract_keywords(query);
        assert!(keywords.contains(&"authentication".to_string()));
        assert!(keywords.contains(&"login".to_string()));
        assert!(keywords.contains(&"bug".to_string()));
        assert!(!keywords.contains(&"how".to_string()));
        assert!(!keywords.contains(&"do".to_string()));
    }

    #[test]
    fn test_extract_keywords_empty() {
        let query = "how do i do this";
        let keywords = extract_keywords(query);
        assert!(keywords.is_empty() || keywords.len() <= 1);
    }

    #[test]
    fn test_context_mode_config_default() {
        let config = ContextModeConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_files, 8);
        assert!((config.min_score - 0.15).abs() < 0.01);
    }

    #[test]
    fn test_detect_lang() {
        assert_eq!(detect_lang("src/main.rs"), "rust");
        assert_eq!(detect_lang("app.py"), "python");
        assert_eq!(detect_lang("main.js"), "javascript");
        assert_eq!(detect_lang("README.md"), "markdown");
    }
}
