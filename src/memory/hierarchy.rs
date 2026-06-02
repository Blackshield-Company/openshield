use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::memory::embeddings::{cosine_similarity, generate_embedding};
use crate::memory::store::{MemoryStore, Message};

/// A layer in the memory hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextLayer {
    /// Current conversation only.
    #[allow(dead_code)]
    Session,
    /// All sessions in the current project/directory.
    #[allow(dead_code)]
    Project,
    /// All sessions across all projects.
    Global,
}

/// Provides hierarchical access to memory: session → project → global.
pub struct MemoryHierarchy<'a> {
    store: &'a MemoryStore,
}

impl<'a> MemoryHierarchy<'a> {
    /// Create a new MemoryHierarchy backed by the given store.
    pub fn new(store: &'a MemoryStore) -> Self {
        Self { store }
    }

    /// Get all messages for a specific session.
    pub fn get_session_context(&self, session_id: &str) -> Result<Vec<Message>> {
        self.store
            .get_session_messages(session_id)
            .context("Failed to get session context")
    }

    /// Get all messages for sessions belonging to a specific project.
    pub fn get_project_context(&self, project_path: &str) -> Result<Vec<Message>> {
        let session_ids = self
            .store
            .get_sessions_by_project(project_path)
            .context("Failed to get sessions by project")?;

        let mut all_messages = Vec::new();
        for session in session_ids {
            let messages = self
                .store
                .get_session_messages(&session.id)
                .context("Failed to get session messages for project context")?;
            all_messages.extend(messages);
        }

        // Sort by creation time for coherent narrative
        all_messages.sort_by_key(|m| m.created_at);
        Ok(all_messages)
    }

    /// Get all messages across all sessions (global context).
    pub fn get_global_context(&self) -> Result<Vec<Message>> {
        let sessions = self
            .store
            .get_recent_sessions(1000)
            .context("Failed to get recent sessions for global context")?;

        let mut all_messages = Vec::new();
        for session in sessions {
            let messages = self
                .store
                .get_session_messages(&session.id)
                .context("Failed to get session messages for global context")?;
            all_messages.extend(messages);
        }

        // Sort by creation time
        all_messages.sort_by_key(|m| m.created_at);
        Ok(all_messages)
    }

    /// Query a specific layer with semantic + keyword search.
    ///
    /// Returns messages ranked by relevance score (higher = more relevant).
    pub fn query_layer(
        &self,
        layer: ContextLayer,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(Message, f32)>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        // Collect candidate messages based on layer
        let candidates = match layer {
            ContextLayer::Session => {
                // For session layer, we need a session_id in the query context
                // This is handled by the caller providing the session_id as part of query
                // or we search globally and filter. For simplicity, search globally here.
                self.store
                    .get_recent_sessions(1000)
                    .context("Failed to get sessions for query")?
                    .into_iter()
                    .flat_map(|s| self.store.get_session_messages(&s.id).unwrap_or_default())
                    .collect::<Vec<_>>()
            }
            ContextLayer::Project => {
                // Extract project path from query or use a default
                // For now, we return empty since project path must be provided externally
                Vec::new()
            }
            ContextLayer::Global => self.get_global_context().unwrap_or_default(),
        };

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        // Compute query embedding for semantic search
        let query_embedding = generate_embedding(query);

        // Score each message with combined semantic + keyword + recency scoring
        let now = Utc::now();
        let mut scored: Vec<(Message, f32)> = candidates
            .into_iter()
            .map(|msg| {
                let semantic_score = self.compute_semantic_score(&msg, &query_embedding);
                let keyword_score = self.compute_keyword_score(&msg, query);
                let recency_score = self.compute_recency_score(&msg, now);

                // Combined score: semantic 50%, keyword 30%, recency 20%
                let combined = semantic_score * 0.5 + keyword_score * 0.3 + recency_score * 0.2;
                (msg, combined)
            })
            .collect();

        // Sort by score descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Deduplicate by content (keep highest scoring)
        let mut seen = std::collections::HashSet::new();
        scored.retain(|(msg, _)| seen.insert(msg.content.clone()));

        scored.truncate(limit);
        Ok(scored)
    }

    /// Query the project layer specifically.
    #[allow(dead_code)]
    pub fn query_project_layer(
        &self,
        project_path: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(Message, f32)>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let candidates = self.get_project_context(project_path).unwrap_or_default();

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let query_embedding = generate_embedding(query);
        let now = Utc::now();

        let mut scored: Vec<(Message, f32)> = candidates
            .into_iter()
            .map(|msg| {
                let semantic_score = self.compute_semantic_score(&msg, &query_embedding);
                let keyword_score = self.compute_keyword_score(&msg, query);
                let recency_score = self.compute_recency_score(&msg, now);
                let combined = semantic_score * 0.5 + keyword_score * 0.3 + recency_score * 0.2;
                (msg, combined)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut seen = std::collections::HashSet::new();
        scored.retain(|(msg, _)| seen.insert(msg.content.clone()));

        scored.truncate(limit);
        Ok(scored)
    }

    /// Query a specific session only.
    #[allow(dead_code)]
    pub fn query_session_layer(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(Message, f32)>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let candidates = self.get_session_context(session_id).unwrap_or_default();

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let query_embedding = generate_embedding(query);
        let now = Utc::now();

        let mut scored: Vec<(Message, f32)> = candidates
            .into_iter()
            .map(|msg| {
                let semantic_score = self.compute_semantic_score(&msg, &query_embedding);
                let keyword_score = self.compute_keyword_score(&msg, query);
                let recency_score = self.compute_recency_score(&msg, now);
                let combined = semantic_score * 0.5 + keyword_score * 0.3 + recency_score * 0.2;
                (msg, combined)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut seen = std::collections::HashSet::new();
        scored.retain(|(msg, _)| seen.insert(msg.content.clone()));

        scored.truncate(limit);
        Ok(scored)
    }

    /// Compute semantic similarity score between a message and query embedding.
    fn compute_semantic_score(&self, msg: &Message, query_embedding: &[f32]) -> f32 {
        let msg_embedding = generate_embedding(&msg.content);
        cosine_similarity(&msg_embedding, query_embedding).max(0.0) // Clamp to [0, 1]
    }

    /// Compute keyword match score.
    fn compute_keyword_score(&self, msg: &Message, query: &str) -> f32 {
        let query_words: Vec<String> = query
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty() && s.len() > 1)
            .map(|s| s.to_string())
            .collect();

        if query_words.is_empty() {
            return 0.0;
        }

        let content_lower = msg.content.to_lowercase();
        let matches = query_words
            .iter()
            .filter(|word| content_lower.contains(*word))
            .count();

        matches as f32 / query_words.len() as f32
    }

    /// Compute recency score: newer messages score higher.
    /// Returns a value in [0, 1] where 1.0 is within the last hour.
    fn compute_recency_score(&self, msg: &Message, now: DateTime<Utc>) -> f32 {
        let age = now.signed_duration_since(msg.created_at);
        let hours_old = age.num_hours() as f32;

        if hours_old <= 0.0 {
            1.0
        } else if hours_old >= 168.0 {
            // Older than a week
            0.0
        } else {
            1.0 - (hours_old / 168.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn create_test_store() -> MemoryStore {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let db_path = format!(
            "/tmp/openshark_hierarchy_test_{}_{}.db",
            std::process::id(),
            count
        );
        let _ = std::fs::remove_file(&db_path);
        MemoryStore::new(Path::new(&db_path)).unwrap()
    }

    fn create_test_message(session_id: &str, id: &str, role: &str, content: &str) -> Message {
        Message {
            id: id.to_string(),
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: Utc::now(),
            tokens_used: Some(100),
        }
    }

    #[test]
    fn test_context_layer_enum() {
        assert_eq!(ContextLayer::Session, ContextLayer::Session);
        assert_ne!(ContextLayer::Session, ContextLayer::Global);
    }

    #[test]
    fn test_get_session_context() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg = create_test_message("sess-1", "msg-1", "user", "Hello");
        store.save_message(&msg).unwrap();

        let context = hierarchy.get_session_context("sess-1").unwrap();
        assert_eq!(context.len(), 1);
        assert_eq!(context[0].content, "Hello");
    }

    #[test]
    fn test_get_session_context_empty() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let context = hierarchy.get_session_context("sess-1").unwrap();
        assert!(context.is_empty());
    }

    #[test]
    fn test_get_project_context() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);
        let project_path = "/home/user/myproject";

        store
            .create_session_with_project("sess-1", "model-a", "code", project_path)
            .unwrap();
        store
            .create_session_with_project("sess-2", "model-b", "chat", project_path)
            .unwrap();
        store
            .create_session_with_project("sess-3", "model-c", "other", "/other/project")
            .unwrap();

        let msg1 = create_test_message("sess-1", "msg-1", "user", "Hello from session 1");
        let msg2 = create_test_message("sess-2", "msg-2", "user", "Hello from session 2");
        let msg3 = create_test_message("sess-3", "msg-3", "user", "Hello from other project");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();
        store.save_message(&msg3).unwrap();

        let context = hierarchy.get_project_context(project_path).unwrap();
        assert_eq!(context.len(), 2);
        assert!(context.iter().any(|m| m.content == "Hello from session 1"));
        assert!(context.iter().any(|m| m.content == "Hello from session 2"));
        assert!(
            !context
                .iter()
                .any(|m| m.content == "Hello from other project")
        );
    }

    #[test]
    fn test_get_global_context() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        store.create_session("sess-2", "model-b", "chat").unwrap();

        let msg1 = create_test_message("sess-1", "msg-1", "user", "Global message 1");
        let msg2 = create_test_message("sess-2", "msg-2", "user", "Global message 2");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let context = hierarchy.get_global_context().unwrap();
        assert_eq!(context.len(), 2);
    }

    #[test]
    fn test_query_layer_global() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg1 = create_test_message("sess-1", "msg-1", "user", "rust programming language");
        let msg2 = create_test_message("sess-1", "msg-2", "user", "python scripting");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let results = hierarchy
            .query_layer(ContextLayer::Global, "rust programming", 5)
            .unwrap();
        assert!(!results.is_empty());
        // Rust message should be top result
        assert!(results[0].0.content.contains("rust"));
    }

    #[test]
    fn test_query_layer_empty_query() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        let results = hierarchy.query_layer(ContextLayer::Global, "", 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_query_layer_zero_limit() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg = create_test_message("sess-1", "msg-1", "user", "test");
        store.save_message(&msg).unwrap();

        let results = hierarchy
            .query_layer(ContextLayer::Global, "test", 0)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_query_session_layer() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg1 = create_test_message("sess-1", "msg-1", "user", "authentication setup");
        let msg2 = create_test_message("sess-1", "msg-2", "user", "database schema");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let results = hierarchy.query_session_layer("sess-1", "auth", 5).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].0.content.contains("authentication"));
    }

    #[test]
    fn test_query_project_layer() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);
        let project_path = "/home/user/auth-project";

        store
            .create_session_with_project("sess-1", "model-a", "code", project_path)
            .unwrap();
        let msg1 = create_test_message("sess-1", "msg-1", "user", "jwt token implementation");
        let msg2 = create_test_message("sess-1", "msg-2", "user", "user profile page");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let results = hierarchy
            .query_project_layer(project_path, "jwt", 5)
            .unwrap();
        assert!(!results.is_empty());
        assert!(results[0].0.content.contains("jwt"));
    }

    #[test]
    fn test_combined_scoring() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg1 = create_test_message("sess-1", "msg-1", "user", "rust programming");
        let msg2 = create_test_message("sess-1", "msg-2", "user", "python programming");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let results = hierarchy
            .query_layer(ContextLayer::Global, "rust", 5)
            .unwrap();
        assert_eq!(results.len(), 2);
        // Rust message should score higher
        assert!(results[0].0.content.contains("rust"));
        assert!(results[0].1 >= results[1].1);
    }

    #[test]
    fn test_deduplication() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        // Save same content twice with different IDs
        let msg1 = create_test_message("sess-1", "msg-1", "user", "duplicate content");
        let msg2 = create_test_message("sess-1", "msg-2", "user", "duplicate content");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let results = hierarchy
            .query_layer(ContextLayer::Global, "duplicate", 5)
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_keyword_score() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        let msg = create_test_message("sess-1", "msg-1", "user", "hello world foo bar");
        let score = hierarchy.compute_keyword_score(&msg, "hello foo");
        assert!(score > 0.0);
        assert!(score <= 1.0);
    }

    #[test]
    fn test_recency_score() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);
        let now = Utc::now();

        let recent_msg = create_test_message("sess-1", "msg-1", "user", "recent");
        let score = hierarchy.compute_recency_score(&recent_msg, now);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_semantic_score() {
        let store = create_test_store();
        let hierarchy = MemoryHierarchy::new(&store);

        let msg = create_test_message("sess-1", "msg-1", "user", "rust programming");
        let query_emb = generate_embedding("rust coding");
        let score = hierarchy.compute_semantic_score(&msg, &query_emb);
        assert!(score > 0.0);
        assert!(score <= 1.0);
    }
}
