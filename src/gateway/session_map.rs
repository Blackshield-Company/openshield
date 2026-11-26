//! Persistent mapping from a chat/thread to a MemoryStore session id.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

/// Key for a conversation on a chat platform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelKey {
    pub platform: String,
    pub chat_id: String,
    pub thread_id: String,
}

impl ChannelKey {
    pub fn new(
        platform: impl Into<String>,
        chat_id: impl Into<String>,
        thread_id: Option<&str>,
    ) -> Self {
        Self {
            platform: platform.into(),
            chat_id: chat_id.into(),
            thread_id: thread_id.unwrap_or("").to_string(),
        }
    }
}

/// SQLite-backed map, stored beside the memory database (or at a given path).
pub struct SessionMap {
    conn: MutexConn,
}

// rusqlite::Connection is not Sync; wrap in a mutex for the supervisor.
struct MutexConn(std::sync::Mutex<Connection>);

impl SessionMap {
    /// Open (or create) the map at `path`. Parent directories are created.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS channel_sessions (
                platform TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                thread_id TEXT NOT NULL DEFAULT '',
                user_id TEXT,
                session_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (platform, chat_id, thread_id)
             );",
        )?;
        Ok(Self {
            conn: MutexConn(std::sync::Mutex::new(conn)),
        })
    }

    /// Default file: same directory as `memory_db_path`, named `channels.db`.
    pub fn open_beside_memory(memory_db_path: &Path) -> Result<Self> {
        let path = match memory_db_path.parent() {
            Some(dir) => dir.join("channels.db"),
            None => PathBuf::from("channels.db"),
        };
        Self::open(&path)
    }

    pub fn get(&self, key: &ChannelKey) -> Result<Option<String>> {
        let conn = self.conn.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT session_id FROM channel_sessions
             WHERE platform = ?1 AND chat_id = ?2 AND thread_id = ?3",
        )?;
        let id = stmt
            .query_row(params![key.platform, key.chat_id, key.thread_id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(id)
    }

    /// Insert or replace the session for this chat/thread.
    pub fn put(&self, key: &ChannelKey, user_id: &str, session_id: &str) -> Result<()> {
        let conn = self.conn.0.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO channel_sessions
                (platform, chat_id, thread_id, user_id, session_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(platform, chat_id, thread_id) DO UPDATE SET
                user_id = excluded.user_id,
                session_id = excluded.session_id,
                created_at = excluded.created_at",
            params![
                key.platform,
                key.chat_id,
                key.thread_id,
                user_id,
                session_id,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn remove(&self, key: &ChannelKey) -> Result<()> {
        let conn = self.conn.0.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "DELETE FROM channel_sessions WHERE platform = ?1 AND chat_id = ?2 AND thread_id = ?3",
            params![key.platform, key.chat_id, key.thread_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_db() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "openshield-channels-map-{}-{n}.db",
            std::process::id()
        ))
    }

    #[test]
    fn put_get_replace_remove() {
        let path = temp_db();
        let _ = std::fs::remove_file(&path);
        let map = SessionMap::open(&path).unwrap();
        let key = ChannelKey::new("telegram", "100", None);
        assert!(map.get(&key).unwrap().is_none());
        map.put(&key, "u1", "sess-a").unwrap();
        assert_eq!(map.get(&key).unwrap().as_deref(), Some("sess-a"));
        map.put(&key, "u1", "sess-b").unwrap();
        assert_eq!(map.get(&key).unwrap().as_deref(), Some("sess-b"));
        let threaded = ChannelKey::new("discord", "chan", Some("thread-1"));
        map.put(&threaded, "u2", "sess-t").unwrap();
        assert_eq!(map.get(&key).unwrap().as_deref(), Some("sess-b"));
        assert_eq!(map.get(&threaded).unwrap().as_deref(), Some("sess-t"));
        map.remove(&key).unwrap();
        assert!(map.get(&key).unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_beside_memory() {
        let dir = std::env::temp_dir().join(format!("openshield-ch-beside-{}", std::process::id()));
        let memory = dir.join("memory.db");
        let map = SessionMap::open_beside_memory(&memory).unwrap();
        let key = ChannelKey::new("telegram", "1", None);
        map.put(&key, "u", "s").unwrap();
        assert!(dir.join("channels.db").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
