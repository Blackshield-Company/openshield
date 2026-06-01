//! Context Compression & Summarization System
//!
//! When a session's context approaches the model's limit, this system
//! compresses older messages into summaries to keep the conversation alive.
//!
//! ## How it works
//!
//! 1. **Monitor** — Track estimated token usage against model context length
//! 2. **Threshold** — User-configurable `compression_threshold` (0.0–1.0, default 0.8)
//! 3. **Trigger** — When `tokens_used / context_length > threshold`, compress
//! 4. **Compress** — Group old messages into chunks, summarize each via LLM
//! 5. **Replace** — Old messages → summary message(s) in `model_messages`
//!
//! ## User Config
//!
//! ```toml
//! [context_compression]
//! enabled = true
//! threshold = 0.8          # Trigger at 80% of context window
//! summary_model = "kimi-k2.6"  # Model to use for summarization
//! preserve_recent = 4      # Keep N most recent exchanges uncompressed
//! ```

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::providers::{ChatRequest, Message as ProviderMessage, Provider};

/// User-configurable context compression settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompressionConfig {
    /// Enable context compression.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Fraction of context window that triggers compression (0.0–1.0).
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// Model to use for generating summaries.
    #[serde(default = "default_summary_model")]
    pub summary_model: String,
    /// Number of most recent user/assistant exchanges to preserve verbatim.
    #[serde(default = "default_preserve_recent")]
    pub preserve_recent: usize,
    /// Maximum tokens per summary chunk.
    #[serde(default = "default_max_summary_tokens")]
    pub max_summary_tokens: usize,
}

impl Default for ContextCompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 0.8,
            summary_model: "kimi-k2.6".to_string(),
            preserve_recent: 4,
            max_summary_tokens: 512,
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_threshold() -> f64 {
    0.8
}

fn default_summary_model() -> String {
    "kimi-k2.6".to_string()
}

fn default_preserve_recent() -> usize {
    4
}

fn default_max_summary_tokens() -> usize {
    512
}

/// Tracks compression statistics for a session.
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    /// How many times compression has run this session.
    pub compressions_done: usize,
    /// Total messages replaced with summaries.
    pub messages_summarized: usize,
    /// Estimated tokens saved.
    pub tokens_saved: usize,
}

/// The context compression engine.
pub struct ContextCompressor {
    config: ContextCompressionConfig,
    stats: CompressionStats,
}

impl ContextCompressor {
    pub fn new(config: ContextCompressionConfig) -> Self {
        Self {
            config,
            stats: CompressionStats::default(),
        }
    }

    /// Check if compression should trigger.
    ///
    /// `estimated_tokens` is a rough count (words / 0.75).
    /// `context_length` is the model's max context window.
    pub fn should_compress(&self, estimated_tokens: usize, context_length: usize) -> bool {
        if !self.config.enabled || context_length == 0 {
            return false;
        }
        let ratio = estimated_tokens as f64 / context_length as f64;
        ratio >= self.config.threshold
    }

    /// Compress `model_messages` in place.
    ///
    /// Preserves the system prompt (index 0) and the N most recent
    /// user/assistant exchanges. Everything in between is summarized.
    ///
    /// Returns `true` if compression occurred.
    pub fn compress(
        &mut self,
        model_messages: &mut Vec<ProviderMessage>,
        _provider: &Provider,
    ) -> Result<bool> {
        if model_messages.len() <= 2 + self.config.preserve_recent * 2 {
            // Not enough history to bother compressing
            return Ok(false);
        }

        // Split: [system] + [compressible] + [preserved]
        let system_msg = if model_messages[0].role == "system" {
            Some(model_messages[0].clone())
        } else {
            None
        };

        let preserve_start = model_messages.len().saturating_sub(self.config.preserve_recent * 2);
        let preserved: Vec<ProviderMessage> = model_messages.split_off(preserve_start);
        let compressible = model_messages.clone();

        let has_system = system_msg.is_some();

        // Clear and rebuild
        model_messages.clear();
        if let Some(sys) = system_msg {
            model_messages.push(sys);
        }

        // Group compressible messages into chunks and summarize
        let chunks = self.chunk_messages(&compressible, has_system);
        let mut summarized_count = 0;

        for chunk in chunks {
            if chunk.len() < 2 {
                // Too small to summarize — keep as-is
                for msg in chunk {
                    model_messages.push(msg);
                }
                continue;
            }

            let summary = self.summarize_chunk(&chunk)?;
            model_messages.push(ProviderMessage {
                role: "system".to_string(),
                content: format!(
                    "[COMPRESSED CONTEXT — {} messages summarized]\n{}",
                    chunk.len(),
                    summary
                ),
                images: None,
            });
            summarized_count += chunk.len();
        }

        // Append preserved messages
        for msg in preserved {
            model_messages.push(msg);
        }

        if summarized_count > 0 {
            self.stats.compressions_done += 1;
            self.stats.messages_summarized += summarized_count;
            // Rough estimate: each message ~100 tokens, summary ~max_summary_tokens
            let saved = summarized_count.saturating_mul(100)
                .saturating_sub(self.config.max_summary_tokens);
            self.stats.tokens_saved += saved;
        }

        Ok(summarized_count > 0)
    }

    /// Split messages into chunks for summarization.
    fn chunk_messages(
        &self,
        messages: &[ProviderMessage],
        has_system: bool,
    ) -> Vec<Vec<ProviderMessage>> {
        let start = if has_system { 1 } else { 0 };
        let relevant = &messages[start..];

        if relevant.is_empty() {
            return Vec::new();
        }

        // Aim for ~6 messages per chunk (3 exchanges)
        const CHUNK_SIZE: usize = 6;
        let mut chunks: Vec<Vec<ProviderMessage>> = Vec::new();
        let mut current = Vec::new();

        for msg in relevant.iter().cloned() {
            current.push(msg);
            if current.len() >= CHUNK_SIZE {
                chunks.push(current);
                current = Vec::new();
            }
        }

        if !current.is_empty() {
            // Merge small final chunk into previous if possible
            if let Some(last) = chunks.last_mut() {
                if last.len() + current.len() <= CHUNK_SIZE + 2 {
                    last.extend(current);
                } else {
                    chunks.push(current);
                }
            } else {
                chunks.push(current);
            }
        }

        chunks
    }

    /// Ask the LLM to summarize a chunk of messages.
    fn summarize_chunk(
        &self,
        chunk: &[ProviderMessage],
    ) -> Result<String> {
        let conversation = chunk
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "Summarize the following conversation concisely. Capture key decisions, \
             code changes, facts learned, and action items. Be brief but complete. \
             Do not use markdown headers.\n\n{}",
            conversation
        );

        let _request = ChatRequest::new(
            self.config.summary_model.clone(),
            vec![
                ProviderMessage {
                    role: "system".to_string(),
                    content: "You are a context compression assistant. Summarize conversations into dense, information-rich paragraphs.".to_string(),
                    images: None,
                },
                ProviderMessage {
                    role: "user".to_string(),
                    content: prompt,
                    images: None,
                },
            ],
            false, // non-streaming for summary
        );

        // We need a provider — but we don't have one in this context.
        // The caller should pass one. For now, return a local summary.
        // This is a placeholder that will be replaced with actual LLM call
        // when the provider is available.
        Ok(self.local_summarize(chunk))
    }

    /// Fallback local summarization when no provider is available.
    fn local_summarize(&self, chunk: &[ProviderMessage]) -> String {
        let mut topics: Vec<String> = Vec::new();
        let mut code_snippets: Vec<String> = Vec::new();
        let mut decisions: Vec<String> = Vec::new();

        for msg in chunk {
            let content = &msg.content;
            // Extract code blocks
            if content.contains("```") {
                let langs: Vec<&str> = content
                    .lines()
                    .filter(|l| l.starts_with("```") && l.len() > 3)
                    .map(|l| l.trim_start_matches("`").trim())
                    .collect();
                for lang in langs {
                    if !lang.is_empty() && lang != "```" {
                        code_snippets.push(format!("{} code", lang));
                    }
                }
            }
            // Extract decisions (imperative statements)
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Set ") || trimmed.starts_with("Changed ") || trimmed.starts_with("Fixed ") || trimmed.starts_with("Added ") || trimmed.starts_with("Removed ") {
                    decisions.push(trimmed.to_string());
                }
            }
            // Simple topic extraction (first sentence)
            if let Some(first_sentence) = content.split('.').next() {
                let topic = first_sentence.trim();
                if topic.len() > 10 && topic.len() < 200 {
                    topics.push(topic.to_string());
                }
            }
        }

        let mut summary = String::new();

        if !topics.is_empty() {
            summary.push_str("Topics: ");
            summary.push_str(&topics.join("; "));
            summary.push('\n');
        }

        if !code_snippets.is_empty() {
            summary.push_str("Code: ");
            // Deduplicate
            let mut seen = std::collections::HashSet::new();
            let unique: Vec<_> = code_snippets.into_iter().filter(|s| seen.insert(s.clone())).collect();
            summary.push_str(&unique.join(", "));
            summary.push('\n');
        }

        if !decisions.is_empty() {
            summary.push_str("Actions: ");
            summary.push_str(&decisions.join("; "));
            summary.push('\n');
        }

        if summary.is_empty() {
            summary.push_str("General discussion and planning.");
        }

        summary.trim().to_string()
    }

    /// Get compression stats.
    pub fn stats(&self) -> &CompressionStats {
        &self.stats
    }

    /// Reset stats (e.g., on session clear).
    pub fn reset_stats(&mut self) {
        self.stats = CompressionStats::default();
    }
}

/// Estimate token count from a message list.
///
/// Uses a rough heuristic: tokens ≈ words / 0.75.
/// This is fast and good enough for triggering compression.
pub fn estimate_tokens(messages: &[ProviderMessage]) -> usize {
    let total_words: usize = messages
        .iter()
        .map(|m| m.content.split_whitespace().count())
        .sum();
    (total_words as f64 / 0.75) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(role: &str, content: &str) -> ProviderMessage {
        ProviderMessage {
            role: role.to_string(),
            content: content.to_string(),
            images: None,
        }
    }

    #[test]
    fn test_estimate_tokens() {
        let messages = vec![
            make_msg("system", "You are a helpful assistant."),
            make_msg("user", "Hello world how are you today"),
            make_msg("assistant", "I am doing well thank you for asking"),
        ];
        let tokens = estimate_tokens(&messages);
        assert!(tokens > 0);
        // 15 words / 0.75 ≈ 20 tokens
        assert!(tokens >= 15 && tokens <= 30);
    }

    #[test]
    fn test_should_compress_triggered() {
        let config = ContextCompressionConfig {
            enabled: true,
            threshold: 0.8,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        assert!(compressor.should_compress(900, 1000)); // 90% > 80%
        assert!(!compressor.should_compress(700, 1000)); // 70% < 80%
    }

    #[test]
    fn test_should_compress_disabled() {
        let config = ContextCompressionConfig {
            enabled: false,
            threshold: 0.5,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        assert!(!compressor.should_compress(900, 1000));
    }

    #[test]
    fn test_chunk_messages() {
        let config = ContextCompressionConfig::default();
        let compressor = ContextCompressor::new(config);

        let messages: Vec<ProviderMessage> = (0..10)
            .map(|i| make_msg(&format!("role{}", i % 2), &format!("message {}", i)))
            .collect();

        let chunks = compressor.chunk_messages(&messages, false);
        assert!(!chunks.is_empty());
        // 10 messages / 6 per chunk = 2 chunks
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_local_summarize() {
        let config = ContextCompressionConfig::default();
        let compressor = ContextCompressor::new(config);

        let chunk = vec![
            make_msg("user", "We need to fix the auth bug. Set the token expiry to 1 hour."),
            make_msg("assistant", "```rust\nlet expiry = Duration::from_secs(3600);\n```\nDone."),
        ];

        let summary = compressor.local_summarize(&chunk);
        assert!(summary.contains("auth") || summary.contains("token"));
    }

    #[test]
    fn test_compression_preserves_system() {
        let config = ContextCompressionConfig {
            enabled: true,
            threshold: 0.8,
            preserve_recent: 1,
            ..Default::default()
        };
        let mut compressor = ContextCompressor::new(config);

        let mut messages = vec![
            make_msg("system", "You are OpenShark."),
            make_msg("user", "Hello"),
            make_msg("assistant", "Hi!"),
            make_msg("user", "What's new?"),
            make_msg("assistant", "Not much."),
            make_msg("user", "Tell me a joke."),
            make_msg("assistant", "Why did the shark cross the ocean? To get to the other tide."),
        ];

        let result = compressor.compress(
            &mut messages,
            &crate::providers::Provider::new(
                "test".to_string(),
                "http://localhost".to_string(),
                "test".to_string(),
                crate::config::ProviderKind::OpenAiCompatible,
                std::collections::HashMap::new(),
            ),
        );

        assert!(result.is_ok());
        let did_compress = result.unwrap();
        assert!(did_compress);

        // System message should still be first
        assert_eq!(messages[0].role, "system");
        assert!(messages[0].content.contains("OpenShark"));

        // Last messages should be preserved
        assert_eq!(messages[messages.len() - 2].role, "user");
        assert_eq!(messages[messages.len() - 1].role, "assistant");
    }
}
