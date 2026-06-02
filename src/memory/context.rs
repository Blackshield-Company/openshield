use anyhow::{Context, Result};
use regex::Regex;

use crate::memory::hierarchy::{ContextLayer, MemoryHierarchy};
use crate::memory::store::{MemoryStore, Message};

/// Injects relevant context from the memory hierarchy into conversations.
pub struct ContextInjector<'a> {
    hierarchy: MemoryHierarchy<'a>,
    store: &'a MemoryStore,
}

/// A natural language query pattern for extracting intent.
#[derive(Debug, Clone)]
enum QueryIntent {
    /// "What did we do about X?" — looking for past actions/decisions
    WhatDidWeDo { topic: String },
    /// "How did we solve X?" — looking for solutions
    HowDidWeSolve { topic: String },
    /// "Tell me about X" — general information request
    TellMeAbout { topic: String },
    /// "What was the issue with X?" — looking for problems/errors
    WhatWasTheIssue { topic: String },
    /// Generic query
    Generic { query: String },
}

impl<'a> ContextInjector<'a> {
    /// Create a new ContextInjector.
    pub fn new(store: &'a MemoryStore) -> Self {
        Self {
            hierarchy: MemoryHierarchy::new(store),
            store,
        }
    }

    /// Inject relevant past messages into the current session based on a query.
    ///
    /// Searches across all layers (session → project → global) and returns
    /// the top 5 most relevant messages that provide context for the query.
    #[allow(dead_code)]
    pub fn inject_relevant_context(&self, query: &str, session_id: &str) -> Result<Vec<Message>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Determine the project path for this session
        let project_path = self
            .get_session_project_path(session_id)
            .unwrap_or_default();

        // Collect results from all layers with different weights
        let mut all_results: Vec<(Message, f32)> = Vec::new();

        // Session layer: highest relevance, search current session
        let session_results = self
            .hierarchy
            .query_session_layer(session_id, query, 5)
            .unwrap_or_default();
        for (msg, score) in session_results {
            all_results.push((msg, score * 1.5)); // Boost session context
        }

        // Project layer: medium relevance
        if !project_path.is_empty() {
            let project_results = self
                .hierarchy
                .query_project_layer(&project_path, query, 5)
                .unwrap_or_default();
            for (msg, score) in project_results {
                // Skip messages already in session layer
                if !all_results.iter().any(|(m, _)| m.id == msg.id) {
                    all_results.push((msg, score * 1.2)); // Boost project context
                }
            }
        }

        // Global layer: lowest relevance but broadest scope
        let global_results = self
            .hierarchy
            .query_layer(ContextLayer::Global, query, 5)
            .unwrap_or_default();
        for (msg, score) in global_results {
            if !all_results.iter().any(|(m, _)| m.id == msg.id) {
                all_results.push((msg, score));
            }
        }

        // Sort by score descending
        all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Limit to top 5 most relevant
        all_results.truncate(5);

        // Extract just the messages
        let messages = all_results.into_iter().map(|(msg, _)| msg).collect();
        Ok(messages)
    }

    /// Answer a natural language query like "What did we do about auth?"
    ///
    /// Parses the query intent, searches relevant context, and returns
    /// a human-readable summary.
    pub fn answer_natural_query(&self, query: &str) -> Result<String> {
        let intent = self.parse_query_intent(query);
        let search_query = self.intent_to_search_query(&intent);

        // Search across all layers
        let global_results = self
            .hierarchy
            .query_layer(ContextLayer::Global, &search_query, 10)
            .unwrap_or_default();

        if global_results.is_empty() {
            return Ok(format!(
                "I couldn't find any relevant information about '{}' in my memory.",
                query
            ));
        }

        // Generate a human-readable answer based on intent and results
        let answer = self.format_natural_answer(&intent, &global_results);
        Ok(answer)
    }

    /// Get the project path for a session.
    fn get_session_project_path(&self, session_id: &str) -> Result<String> {
        let sessions = self
            .store
            .get_recent_sessions(1000)
            .context("Failed to get sessions")?;

        sessions
            .into_iter()
            .find(|s| s.id == session_id)
            .and_then(|s| s.project_path.clone())
            .ok_or_else(|| anyhow::anyhow!("Session not found or has no project path"))
    }

    /// Parse natural language query into structured intent.
    fn parse_query_intent(&self, query: &str) -> QueryIntent {
        let query_lower = query.to_lowercase();

        // Pattern: "What did we do about X?"
        let what_did_we_do =
            Regex::new(r"what did we do (about|regarding|with|for)\s+(.+?)\??$").expect("valid regex");
        if let Some(caps) = what_did_we_do.captures(&query_lower) {
            return QueryIntent::WhatDidWeDo {
                topic: caps
                    .get(2)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| query.to_string()),
            };
        }

        // Pattern: "How did we solve X?"
        let how_did_we_solve =
            Regex::new(r"how did we (solve|fix|handle|address|implement)\s+(.+?)\??$").expect("valid regex");
        if let Some(caps) = how_did_we_solve.captures(&query_lower) {
            return QueryIntent::HowDidWeSolve {
                topic: caps
                    .get(2)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| query.to_string()),
            };
        }

        // Pattern: "Tell me about X"
        let tell_me_about = Regex::new(r"tell me (about|regarding)\s+(.+?)\??$").expect("valid regex");
        if let Some(caps) = tell_me_about.captures(&query_lower) {
            return QueryIntent::TellMeAbout {
                topic: caps
                    .get(2)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| query.to_string()),
            };
        }

        // Pattern: "What was the issue with X?"
        let what_was_issue =
            Regex::new(r"what (was|is) the (issue|problem|error|bug) (with|in)\s+(.+?)\??$")
                .expect("valid regex");
        if let Some(caps) = what_was_issue.captures(&query_lower) {
            return QueryIntent::WhatWasTheIssue {
                topic: caps
                    .get(4)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| query.to_string()),
            };
        }

        // Fallback to generic
        QueryIntent::Generic {
            query: query.to_string(),
        }
    }

    /// Convert parsed intent into an optimized search query.
    fn intent_to_search_query(&self, intent: &QueryIntent) -> String {
        match intent {
            QueryIntent::WhatDidWeDo { topic } => {
                format!("{} implementation solution decision", topic)
            }
            QueryIntent::HowDidWeSolve { topic } => {
                format!("{} fix solution workaround resolved", topic)
            }
            QueryIntent::TellMeAbout { topic } => topic.clone(),
            QueryIntent::WhatWasTheIssue { topic } => {
                format!("{} error bug problem failed issue", topic)
            }
            QueryIntent::Generic { query } => query.clone(),
        }
    }

    /// Format search results into a natural language answer.
    fn format_natural_answer(&self, intent: &QueryIntent, results: &[(Message, f32)]) -> String {
        let mut answer = String::new();

        match intent {
            QueryIntent::WhatDidWeDo { topic } => {
                answer.push_str(&format!("Here's what we did regarding **{}**:\n\n", topic));
            }
            QueryIntent::HowDidWeSolve { topic } => {
                answer.push_str(&format!("Here's how we addressed **{}**:\n\n", topic));
            }
            QueryIntent::TellMeAbout { topic } => {
                answer.push_str(&format!("Here's what I found about **{}**:\n\n", topic));
            }
            QueryIntent::WhatWasTheIssue { topic } => {
                answer.push_str(&format!(
                    "Here's what I found about issues with **{}**:\n\n",
                    topic
                ));
            }
            QueryIntent::Generic { query } => {
                answer.push_str(&format!("Here's what I found for **{}**:\n\n", query));
            }
        }

        for (i, (msg, _score)) in results.iter().take(5).enumerate() {
            let preview = if msg.content.len() > 200 {
                format!("{}...", &msg.content[..200])
            } else {
                msg.content.clone()
            };

            answer.push_str(&format!(
                "{}. [{}] {}: {}\n",
                i + 1,
                msg.role,
                msg.created_at.format("%Y-%m-%d %H:%M"),
                preview
            ));
        }

        answer.push_str(&format!(
            "\n(Found {} relevant messages)",
            results.len().min(5)
        ));
        answer
    }

    /// Get context summary for display in TUI.
    pub fn get_context_summary(&self, session_id: &str) -> Result<String> {
        let session_msgs = self
            .hierarchy
            .get_session_context(session_id)
            .unwrap_or_default();

        let project_path = self
            .get_session_project_path(session_id)
            .unwrap_or_default();
        let project_msgs = if !project_path.is_empty() {
            self.hierarchy
                .get_project_context(&project_path)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let global_msgs = self.hierarchy.get_global_context().unwrap_or_default();

        let mut summary = String::from("Memory Context Summary\n");
        summary.push_str("======================\n\n");
        summary.push_str(&format!("Session: {} messages\n", session_msgs.len()));
        summary.push_str(&format!(
            "Project ({}): {} messages\n",
            if project_path.is_empty() {
                "none"
            } else {
                &project_path
            },
            project_msgs.len()
        ));
        summary.push_str(&format!("Global: {} messages\n", global_msgs.len()));

        if !session_msgs.is_empty() {
            summary.push_str("\nRecent session messages:\n");
            for msg in session_msgs.iter().rev().take(3) {
                let preview = &msg.content[..msg.content.len().min(60)];
                summary.push_str(&format!("  [{}] {}\n", msg.role, preview));
            }
        }

        Ok(summary)
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
            "/tmp/openshark_context_test_{}_{}.db",
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
            created_at: chrono::Utc::now(),
            tokens_used: Some(100),
        }
    }

    #[test]
    fn test_inject_relevant_context_empty_query() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        let results = injector.inject_relevant_context("", "sess-1").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_inject_relevant_context_no_session() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        let results = injector
            .inject_relevant_context("rust", "nonexistent")
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_inject_relevant_context_basic() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg = create_test_message("sess-1", "msg-1", "user", "rust programming");
        store.save_message(&msg).unwrap();

        let results = injector.inject_relevant_context("rust", "sess-1").unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("rust"));
    }

    #[test]
    fn test_inject_limits_to_five() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        for i in 0..10 {
            let msg = create_test_message(
                "sess-1",
                &format!("msg-{}", i),
                "user",
                &format!("rust topic {}", i),
            );
            store.save_message(&msg).unwrap();
        }

        let results = injector.inject_relevant_context("rust", "sess-1").unwrap();
        assert!(
            results.len() <= 5,
            "Should limit to 5 results, got {}",
            results.len()
        );
    }

    #[test]
    fn test_answer_natural_query_what_did_we_do() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg = create_test_message(
            "sess-1",
            "msg-1",
            "assistant",
            "We implemented JWT authentication with refresh tokens",
        );
        store.save_message(&msg).unwrap();

        let answer = injector
            .answer_natural_query("What did we do about auth?")
            .unwrap();
        assert!(
            answer.contains("auth") || answer.contains("authentication") || answer.contains("JWT")
        );
    }

    #[test]
    fn test_answer_natural_query_how_solve() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg = create_test_message(
            "sess-1",
            "msg-1",
            "assistant",
            "We fixed the database connection by increasing the pool size",
        );
        store.save_message(&msg).unwrap();

        let answer = injector
            .answer_natural_query("How did we solve the database issue?")
            .unwrap();
        assert!(answer.contains("database") || answer.contains("pool"));
    }

    #[test]
    fn test_answer_natural_query_empty() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        let answer = injector
            .answer_natural_query("What did we do about quantum physics?")
            .unwrap();
        assert!(answer.contains("couldn't find") || answer.contains("relevant"));
    }

    #[test]
    fn test_parse_query_intent_what_did_we_do() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        let intent = injector.parse_query_intent("What did we do about authentication?");
        match intent {
            QueryIntent::WhatDidWeDo { topic } => {
                assert!(topic.contains("authentication"));
            }
            _ => panic!("Expected WhatDidWeDo intent, got {:?}", intent),
        }
    }

    #[test]
    fn test_parse_query_intent_how_solve() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        let intent = injector.parse_query_intent("How did we solve the memory leak?");
        match intent {
            QueryIntent::HowDidWeSolve { topic } => {
                assert!(topic.contains("memory leak"));
            }
            _ => panic!("Expected HowDidWeSolve intent, got {:?}", intent),
        }
    }

    #[test]
    fn test_parse_query_intent_tell_me_about() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        let intent = injector.parse_query_intent("Tell me about the API design");
        match intent {
            QueryIntent::TellMeAbout { topic } => {
                assert!(topic.contains("api design"));
            }
            _ => panic!("Expected TellMeAbout intent, got {:?}", intent),
        }
    }

    #[test]
    fn test_parse_query_intent_what_was_issue() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        let intent = injector.parse_query_intent("What was the issue with the build?");
        match intent {
            QueryIntent::WhatWasTheIssue { topic } => {
                assert!(topic.contains("build"));
            }
            _ => panic!("Expected WhatWasTheIssue intent, got {:?}", intent),
        }
    }

    #[test]
    fn test_parse_query_intent_generic() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        let intent = injector.parse_query_intent("random query without pattern");
        match intent {
            QueryIntent::Generic { query } => {
                assert_eq!(query, "random query without pattern");
            }
            _ => panic!("Expected Generic intent, got {:?}", intent),
        }
    }

    #[test]
    fn test_context_summary() {
        let store = create_test_store();
        let injector = ContextInjector::new(&store);

        store.create_session("sess-1", "model-a", "code").unwrap();
        let msg = create_test_message("sess-1", "msg-1", "user", "Hello world");
        store.save_message(&msg).unwrap();

        let summary = injector.get_context_summary("sess-1").unwrap();
        assert!(summary.contains("Session:"));
        assert!(summary.contains("Global:"));
    }
}
