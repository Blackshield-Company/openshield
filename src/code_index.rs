//! Code Index — Persistent SQLite-backed symbol index with background refresh.
//!
//! Provides instant symbol lookup across the codebase.

#![allow(dead_code)]

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A single indexed symbol.
#[derive(Debug, Clone)]
pub struct IndexedSymbol {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub context: String,
}

/// Persistent code index backed by SQLite.
pub struct CodeIndex {
    db: Arc<Mutex<rusqlite::Connection>>,
    root: String,
    last_refresh: Arc<Mutex<u64>>,
}

impl CodeIndex {
    /// Open or create the code index at the given database path.
    pub fn open(db_path: &str, root: &str) -> Result<Self> {
        let conn = rusqlite::Connection::open(db_path)
            .with_context(|| format!("Failed to open code index DB: {}", db_path))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS symbols (
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                context TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT
            )",
            [],
        )?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            root: root.to_string(),
            last_refresh: Arc::new(Mutex::new(now)),
        })
    }

    /// Build the index from scratch by scanning the repo.
    pub fn rebuild(&self) -> Result<usize> {
        let map = crate::repo_map::build_repo_map(&self.root)?;

        let mut conn = self.db.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM symbols", [])?;

        for sym in &map.symbols {
            tx.execute(
                "INSERT INTO symbols (name, kind, file, line, context)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    &sym.name,
                    &sym.kind.to_string(),
                    &sym.file,
                    sym.line as i64,
                    &sym.context
                ],
            )?;
        }

        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('last_refresh', ?1)",
            [map.symbols.len().to_string()],
        )?;

        tx.commit()?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        *self.last_refresh.lock().unwrap() = now;

        Ok(map.symbols.len())
    }

    /// Search symbols by name (partial match).
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<IndexedSymbol>> {
        let pattern = format!("%{}%", query);
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT name, kind, file, line, context FROM symbols
             WHERE name LIKE ?1
             ORDER BY name
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], |row| {
            Ok(IndexedSymbol {
                name: row.get(0)?,
                kind: row.get(1)?,
                file: row.get(2)?,
                line: row.get::<_, i64>(3)? as usize,
                context: row.get(4)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get all symbols in a file.
    pub fn symbols_in_file(&self, file: &str) -> Result<Vec<IndexedSymbol>> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT name, kind, file, line, context FROM symbols
             WHERE file = ?1
             ORDER BY line",
        )?;

        let rows = stmt.query_map([file], |row| {
            Ok(IndexedSymbol {
                name: row.get(0)?,
                kind: row.get(1)?,
                file: row.get(2)?,
                line: row.get::<_, i64>(3)? as usize,
                context: row.get(4)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get index statistics.
    pub fn stats(&self) -> Result<(usize, u64)> {
        let conn = self.db.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
            .unwrap_or(0);
        let last_refresh = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'last_refresh'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_default()
            .parse::<u64>()
            .unwrap_or(0);
        Ok((count as usize, last_refresh))
    }

    /// Spawn a background thread that refreshes the index every `interval`.
    pub fn spawn_background_refresh(self: &Arc<Self>, interval: Duration) {
        let this = Arc::clone(self);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(interval);
                if let Err(e) = this.rebuild() {
                    tracing::warn!("Background code index refresh failed: {}", e);
                } else {
                    tracing::info!("Background code index refreshed");
                }
            }
        });
    }
}

/// Format search results for display.
pub fn format_search_results(query: &str, results: &[IndexedSymbol]) -> String {
    if results.is_empty() {
        return format!("🔍 No symbols found matching '{}'", query);
    }

    let mut lines = vec![
        format!("🔍 Symbol Search: '{}' ({} results)", query, results.len()),
        "─".repeat(60),
    ];

    for sym in results {
        lines.push(format!(
            "  {:10} {:25} → {}:{}",
            sym.kind, sym.name, sym.file, sym.line
        ));
        if !sym.context.is_empty() {
            lines.push(format!("             {}", sym.context));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("/tmp/openshark_code_index_test_{}_{}.db", std::process::id(), id)
    }

    #[test]
    fn test_code_index_lifecycle() {
        let db_path = temp_db();
        let _ = std::fs::remove_file(&db_path);

        let index = CodeIndex::open(&db_path, ".").unwrap();
        let count = index.rebuild().unwrap();
        assert!(count > 0);

        let results = index.search("main", 10).unwrap();
        assert!(!results.is_empty());

        let (stats_count, _) = index.stats().unwrap();
        assert_eq!(stats_count, count);

        let _ = std::fs::remove_file(&db_path);
    }
}
