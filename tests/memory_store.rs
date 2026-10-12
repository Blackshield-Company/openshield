//! SQLite memory store: sessions, messages, tool calls, keyword + semantic search.

mod common;

use chrono::{Duration, Utc};
use common::scratch_dir;
use openshield::memory::{MemoryStore, Message, ToolCall};

fn msg(session: &str, role: &str, content: &str, age_secs: i64) -> Message {
    Message {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session.to_string(),
        role: role.to_string(),
        content: content.to_string(),
        created_at: Utc::now() - Duration::seconds(age_secs),
        tokens_used: Some(5),
    }
}

fn store() -> (MemoryStore, std::path::PathBuf) {
    let dir = scratch_dir("memory");
    let db = dir.join("nested").join("memory.db");
    (MemoryStore::new(&db).expect("open store"), db)
}

#[test]
fn creates_database_and_parent_dirs() {
    let (_s, db) = store();
    assert!(db.exists());
}

#[test]
fn sessions_round_trip_and_archive() {
    let (s, _) = store();
    s.create_session("s1", "gpt-x", "coding").unwrap();
    s.create_session("s2", "gpt-y", "chat").unwrap();
    let got = s.get_session("s1").unwrap().expect("session exists");
    assert_eq!(got.model, "gpt-x");
    assert_eq!(got.task_type, "coding");
    assert!(!got.archived);
    assert!(s.get_session("missing").unwrap().is_none());

    // ensure_session is idempotent.
    s.ensure_session("s1", "gpt-x", "coding").unwrap();
    assert_eq!(s.get_recent_sessions(10).unwrap().len(), 2);

    s.archive_session("s1").unwrap();
    let recent: Vec<_> = s
        .get_recent_sessions(10)
        .unwrap()
        .into_iter()
        .map(|x| x.id)
        .collect();
    assert_eq!(recent, vec!["s2".to_string()]);
    assert_eq!(s.get_archived_sessions(10).unwrap().len(), 1);
    s.unarchive_session("s1").unwrap();
    assert_eq!(s.get_recent_sessions(10).unwrap().len(), 2);
}

#[test]
fn messages_persist_across_reopen_in_order() {
    let (s, db) = store();
    s.create_session("s", "m", "chat").unwrap();
    s.save_message(&msg("s", "user", "first", 30)).unwrap();
    s.save_message(&msg("s", "assistant", "second", 20))
        .unwrap();
    s.save_message(&msg("s", "user", "third", 10)).unwrap();
    drop(s);

    let reopened = MemoryStore::new(&db).unwrap();
    let msgs = reopened.get_session_messages("s").unwrap();
    let contents: Vec<_> = msgs.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(contents, ["first", "second", "third"]);
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[0].tokens_used, Some(5));
}

#[test]
fn keyword_search_matches_substrings_newest_first_with_limit() {
    let (s, _) = store();
    s.create_session("a", "m", "chat").unwrap();
    s.create_session("b", "m", "chat").unwrap();
    s.save_message(&msg("a", "user", "configure the sqlite backend", 300))
        .unwrap();
    s.save_message(&msg("b", "user", "SQLite WAL mode tuning", 200))
        .unwrap();
    s.save_message(&msg("b", "user", "unrelated tui theme", 100))
        .unwrap();

    let hits = s.search_messages("sqlite", 10).unwrap();
    assert_eq!(hits.len(), 2, "LIKE search is case-insensitive for ASCII");
    assert_eq!(hits[0].content, "SQLite WAL mode tuning", "newest first");

    assert_eq!(s.search_messages("sqlite", 1).unwrap().len(), 1);
    assert!(
        s.search_messages("nonexistent-term", 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn keyword_search_is_injection_safe() {
    let (s, _) = store();
    s.create_session("a", "m", "chat").unwrap();
    s.save_message(&msg("a", "user", "it's a quote ' test", 1))
        .unwrap();
    assert_eq!(s.search_messages("'", 10).unwrap().len(), 1);
    assert!(
        s.search_messages("'; DROP TABLE messages; --", 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.get_session_messages("a").unwrap().len(), 1);
}

#[test]
fn semantic_search_ranks_related_content_higher() {
    let (s, _) = store();
    s.create_session("a", "m", "chat").unwrap();
    s.save_message(&msg(
        "a",
        "user",
        "rust borrow checker lifetime error in async function",
        3,
    ))
    .unwrap();
    s.save_message(&msg("a", "user", "bake bread with sourdough starter", 2))
        .unwrap();

    let results = s
        .semantic_search("borrow checker lifetime error rust", 5)
        .unwrap();
    assert!(!results.is_empty());
    assert!(results[0].0.content.contains("borrow checker"));
    for w in results.windows(2) {
        assert!(w[0].1 >= w[1].1, "results sorted by similarity");
    }
    assert!(s.semantic_search("rust", 1).unwrap().len() <= 1);
}

#[test]
fn tool_calls_are_scoped_to_session() {
    let (s, _) = store();
    s.create_session("a", "m", "coding").unwrap();
    s.create_session("b", "m", "coding").unwrap();
    for (sid, tool, ok) in [
        ("a", "fs", true),
        ("a", "git", false),
        ("b", "terminal", true),
    ] {
        s.save_tool_call(&ToolCall {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: sid.into(),
            tool_name: tool.into(),
            args: "args".into(),
            result: "result".into(),
            success: ok,
            created_at: Utc::now(),
        })
        .unwrap();
    }
    let a = s.search_tool_calls_by_session("a").unwrap();
    assert_eq!(a.len(), 2);
    assert!(a.iter().any(|c| c.tool_name == "git" && !c.success));
    assert_eq!(s.search_tool_calls_by_session("b").unwrap().len(), 1);
    assert!(s.search_tool_calls_by_session("zzz").unwrap().is_empty());
}

#[test]
fn analysis_results_upsert() {
    let (s, _) = store();
    s.save_analysis_result("cat", "k", "v1").unwrap();
    assert_eq!(
        s.get_analysis_result("cat", "k").unwrap().as_deref(),
        Some("v1")
    );
    s.save_analysis_result("cat", "k", "v2").unwrap();
    assert_eq!(
        s.get_analysis_result("cat", "k").unwrap().as_deref(),
        Some("v2")
    );
    assert!(s.get_analysis_result("cat", "missing").unwrap().is_none());
}
