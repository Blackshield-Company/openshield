use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::memory::embeddings::{cosine_similarity, generate_embedding};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    #[allow(dead_code)]
    pub started_at: DateTime<Utc>,
    pub model: String,
    pub task_type: String,
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
    pub tokens_used: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub args: String,
    pub result: String,
    pub success: bool,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
}

pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    pub fn new(db_path: &Path) -> Result<Self> {
        std::fs::create_dir_all(db_path.parent().unwrap_or(Path::new(".")))
            .context("Failed to create memory directory")?;

        let conn = Connection::open(db_path).context("Failed to open memory database")?;

        let store = Self { conn };
        store.init_tables()?;
        Ok(store)
    }

    fn init_tables(&self) -> Result<()> {
        // Migration: add project_path column if it doesn't exist (for existing DBs)
        let _ = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN project_path TEXT", []);

        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                started_at TEXT NOT NULL,
                model TEXT NOT NULL,
                task_type TEXT NOT NULL DEFAULT 'general',
                project_path TEXT
            );
            
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                tokens_used INTEGER,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );
            
            CREATE TABLE IF NOT EXISTS tool_calls (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                args TEXT NOT NULL,
                result TEXT NOT NULL,
                success INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );

            CREATE TABLE IF NOT EXISTS analysis_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                category TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(category, key)
            );
            
            CREATE TABLE IF NOT EXISTS message_embeddings (
                message_id TEXT PRIMARY KEY,
                embedding_json TEXT NOT NULL,
                FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS performance_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                metric_type TEXT NOT NULL,
                metric_name TEXT NOT NULL,
                value_ms INTEGER NOT NULL,
                metadata TEXT,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
            CREATE INDEX IF NOT EXISTS idx_messages_content ON messages(content);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_session ON tool_calls(session_id);
            CREATE INDEX IF NOT EXISTS idx_analysis_results_category ON analysis_results(category);
            CREATE INDEX IF NOT EXISTS idx_perf_metrics_type ON performance_metrics(metric_type);
            CREATE INDEX IF NOT EXISTS idx_perf_metrics_time ON performance_metrics(created_at);
            ",
            )
            .context("Failed to create tables")?;

        Ok(())
    }

    pub fn create_session(&self, id: &str, model: &str, task_type: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, started_at, model, task_type, project_path) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, Utc::now().to_rfc3339(), model, task_type, None::<String>],
        ).context("Failed to create session")?;
        Ok(())
    }

    pub fn create_session_with_project(
        &self,
        id: &str,
        model: &str,
        task_type: &str,
        project_path: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, started_at, model, task_type, project_path) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, Utc::now().to_rfc3339(), model, task_type, project_path],
        ).context("Failed to create session with project")?;
        Ok(())
    }

    pub fn get_sessions_by_project(&self, project_path: &str) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, model, task_type, project_path
             FROM sessions
             WHERE project_path = ?1
             ORDER BY started_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![project_path], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    started_at: row
                        .get::<_, String>(1)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    model: row.get(2)?,
                    task_type: row.get(3)?,
                    project_path: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get sessions by project")?;

        Ok(sessions)
    }

    #[allow(dead_code)]
    pub fn search_messages_by_project(
        &self,
        query: &str,
        project_path: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.session_id, m.role, m.content, m.created_at, m.tokens_used
             FROM messages m
             JOIN sessions s ON m.session_id = s.id
             WHERE m.content LIKE ?1 AND s.project_path = ?2
             ORDER BY m.created_at DESC
             LIMIT ?3",
        )?;

        let messages = stmt
            .query_map(params![pattern, project_path, limit as i64], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row
                        .get::<_, String>(4)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    tokens_used: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to search messages by project")?;

        Ok(messages)
    }

    pub fn save_message(&self, msg: &Message) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO messages (id, session_id, role, content, created_at, tokens_used) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    msg.id,
                    msg.session_id,
                    msg.role,
                    msg.content,
                    msg.created_at.to_rfc3339(),
                    msg.tokens_used
                ],
            )
            .context("Failed to save message")?;

        let embedding = generate_embedding(&msg.content);
        let embedding_json =
            serde_json::to_string(&embedding).context("Failed to serialize embedding")?;
        self.conn.execute(
            "INSERT OR REPLACE INTO message_embeddings (message_id, embedding_json) VALUES (?1, ?2)",
            params![msg.id, embedding_json],
        ).context("Failed to save message embedding")?;

        Ok(())
    }

    pub fn get_session_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created_at, tokens_used 
             FROM messages 
             WHERE session_id = ?1 
             ORDER BY created_at",
        )?;

        let messages = stmt
            .query_map(params![session_id], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row
                        .get::<_, String>(4)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    tokens_used: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect messages")?;

        Ok(messages)
    }

    pub fn save_tool_call(&self, call: &ToolCall) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tool_calls (id, session_id, tool_name, args, result, success, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                call.id,
                call.session_id,
                call.tool_name,
                call.args,
                call.result,
                if call.success { 1 } else { 0 },
                call.created_at.to_rfc3339(),
            ],
        ).context("Failed to save tool call")?;
        Ok(())
    }

    pub fn search_messages(&self, query: &str, limit: usize) -> Result<Vec<Message>> {
        let pattern = format!("%{query}%");
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created_at, tokens_used 
             FROM messages 
             WHERE content LIKE ?1 
             ORDER BY created_at DESC 
             LIMIT ?2",
        )?;

        let messages = stmt
            .query_map(params![pattern, limit as i64], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row
                        .get::<_, String>(4)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    tokens_used: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to search messages")?;

        Ok(messages)
    }

    pub fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<(Message, f32)>> {
        let query_embedding = generate_embedding(query);

        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.session_id, m.role, m.content, m.created_at, m.tokens_used, e.embedding_json
             FROM messages m
             JOIN message_embeddings e ON m.id = e.message_id"
        )?;

        let rows = stmt.query_map([], |row| {
            let embedding_json: String = row.get(6)?;
            let embedding: Vec<f32> = serde_json::from_str(&embedding_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let msg = Message {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row
                    .get::<_, String>(4)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
                tokens_used: row.get(5)?,
            };
            Ok((msg, embedding))
        })?;

        let mut scored: Vec<(Message, f32)> = Vec::new();
        for row in rows {
            let (msg, embedding) = row.context("Failed to read message embedding row")?;
            let similarity = cosine_similarity(&query_embedding, &embedding);
            if similarity > 0.0 {
                scored.push((msg, similarity));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        Ok(scored)
    }

    pub fn get_recent_sessions(&self, limit: usize) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, model, task_type, project_path
             FROM sessions
             ORDER BY started_at DESC
             LIMIT ?1",
        )?;

        let sessions = stmt
            .query_map(params![limit as i64], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    started_at: row
                        .get::<_, String>(1)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    model: row.get(2)?,
                    task_type: row.get(3)?,
                    project_path: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get sessions")?;

        Ok(sessions)
    }

    pub fn search_tool_calls_by_session(&self, session_id: &str) -> Result<Vec<ToolCall>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, tool_name, args, result, success, created_at 
             FROM tool_calls 
             WHERE session_id = ?1 
             ORDER BY created_at",
        )?;

        let calls = stmt
            .query_map(params![session_id], |row| {
                Ok(ToolCall {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    args: row.get(3)?,
                    result: row.get(4)?,
                    success: row.get::<_, i32>(5)? != 0,
                    created_at: row
                        .get::<_, String>(6)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to search tool calls")?;

        Ok(calls)
    }

    pub fn save_analysis_result(&self, category: &str, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO analysis_results (category, key, value, created_at)
             VALUES (?1, ?2, ?3, ?4)",
                params![category, key, value, Utc::now().to_rfc3339()],
            )
            .context("Failed to save analysis result")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_analysis_results(
        &self,
        category: &str,
    ) -> Result<Vec<(String, String, DateTime<Utc>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value, created_at FROM analysis_results WHERE category = ?1 ORDER BY created_at DESC"
        )?;
        let results = stmt
            .query_map(params![category], |row| {
                let created_at: String = row.get(2)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    created_at.parse().unwrap_or_else(|_| Utc::now()),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get analysis results")?;
        Ok(results)
    }

    /// Get a single analysis result by category and key.
    pub fn get_analysis_result(&self, category: &str, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT value FROM analysis_results WHERE category = ?1 AND key = ?2 ORDER BY created_at DESC LIMIT 1"
        )?;
        let result = stmt
            .query_map(params![category, key], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get analysis result")?;
        Ok(result.into_iter().next())
    }

    #[allow(dead_code)]
    pub fn get_all_tool_calls_with_sessions(
        &self,
        limit: usize,
    ) -> Result<Vec<(ToolCall, Session)>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                tc.id, tc.session_id, tc.tool_name, tc.args, tc.result, tc.success, tc.created_at,
                s.id, s.started_at, s.model, s.task_type, s.project_path
             FROM tool_calls tc
             JOIN sessions s ON tc.session_id = s.id
             ORDER BY tc.created_at DESC
             LIMIT ?1",
        )?;

        let results = stmt
            .query_map(params![limit as i64], |row| {
                let tool_call = ToolCall {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    args: row.get(3)?,
                    result: row.get(4)?,
                    success: row.get::<_, i32>(5)? != 0,
                    created_at: row
                        .get::<_, String>(6)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                };
                let session = Session {
                    id: row.get(7)?,
                    started_at: row
                        .get::<_, String>(8)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    model: row.get(9)?,
                    task_type: row.get(10)?,
                    project_path: row.get(11)?,
                };
                Ok((tool_call, session))
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get tool calls with sessions")?;

        Ok(results)
    }

    pub fn get_tool_failure_patterns(&self, limit: usize) -> Result<Vec<(String, String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT 
                a.tool_name as tool_a,
                b.tool_name as tool_b,
                COUNT(*) as co_fail_count
             FROM tool_calls a
             JOIN tool_calls b ON a.session_id = b.session_id
             WHERE a.success = 0 AND b.success = 0 AND a.tool_name < b.tool_name
             GROUP BY tool_a, tool_b
             HAVING co_fail_count >= 2
             ORDER BY co_fail_count DESC
             LIMIT ?1",
        )?;

        let patterns = stmt
            .query_map(params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get tool failure patterns")?;

        Ok(patterns)
    }

    pub fn get_common_errors(&self, limit: usize) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT result, COUNT(*) as error_count
             FROM tool_calls
             WHERE success = 0
             GROUP BY result
             HAVING error_count >= 1
             ORDER BY error_count DESC
             LIMIT ?1",
        )?;

        let errors = stmt
            .query_map(params![limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get common errors")?;

        Ok(errors)
    }

    pub fn get_prompt_effectiveness(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, usize, usize, f64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT 
                m.content as prompt,
                COUNT(tc.id) as total_calls,
                SUM(CASE WHEN tc.success = 1 THEN 1 ELSE 0 END) as success_calls
             FROM messages m
             JOIN sessions s ON m.session_id = s.id
             JOIN tool_calls tc ON s.id = tc.session_id
             WHERE m.role = 'system'
             GROUP BY m.content
             HAVING total_calls >= 2
             ORDER BY total_calls DESC
             LIMIT ?1",
        )?;

        let effectiveness = stmt
            .query_map(params![limit as i64], |row| {
                let prompt: String = row.get(0)?;
                let total_calls: usize = row.get(1)?;
                let success_calls: usize = row.get(2)?;
                let success_rate = if total_calls > 0 {
                    (success_calls as f64 / total_calls as f64) * 100.0
                } else {
                    0.0
                };
                Ok((prompt, total_calls, success_calls, success_rate))
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get prompt effectiveness")?;

        Ok(effectiveness)
    }

    pub fn get_session_quality_metrics(&self, limit: usize) -> Result<Vec<SessionQualityMetrics>> {
        let mut stmt = self.conn.prepare(
            "SELECT 
                s.id,
                s.model,
                s.task_type,
                s.started_at,
                COALESCE(msg_counts.message_count, 0) as message_count,
                COALESCE(tc_counts.tool_call_count, 0) as tool_call_count,
                COALESCE(tc_counts.tool_success_count, 0) as tool_success_count
             FROM sessions s
             LEFT JOIN (
                 SELECT session_id, COUNT(*) as message_count
                 FROM messages
                 GROUP BY session_id
             ) msg_counts ON s.id = msg_counts.session_id
             LEFT JOIN (
                 SELECT 
                     session_id, 
                     COUNT(*) as tool_call_count,
                     SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as tool_success_count
                 FROM tool_calls
                 GROUP BY session_id
             ) tc_counts ON s.id = tc_counts.session_id
             ORDER BY s.started_at DESC
             LIMIT ?1",
        )?;

        let metrics = stmt
            .query_map(params![limit as i64], |row| {
                let tool_call_count: usize = row.get(5)?;
                let tool_success_count: usize = row.get(6)?;
                let tool_success_rate = if tool_call_count > 0 {
                    (tool_success_count as f64 / tool_call_count as f64) * 100.0
                } else {
                    0.0
                };

                Ok(SessionQualityMetrics {
                    session_id: row.get(0)?,
                    model: row.get(1)?,
                    task_type: row.get(2)?,
                    started_at: row
                        .get::<_, String>(3)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    message_count: row.get(4)?,
                    tool_call_count,
                    tool_success_count,
                    tool_success_rate,
                    quality_score: 0.0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get session quality metrics")?;

        Ok(metrics)
    }

    pub fn get_model_performance_trends(&self, limit: usize) -> Result<Vec<ModelTrendData>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                s.model,
                date(s.started_at) as day,
                COUNT(DISTINCT s.id) as session_count,
                COUNT(tc.id) as total_calls,
                SUM(CASE WHEN tc.success = 1 THEN 1 ELSE 0 END) as success_calls
             FROM sessions s
             LEFT JOIN tool_calls tc ON s.id = tc.session_id
             GROUP BY s.model, day
             HAVING total_calls > 0
             ORDER BY day DESC, s.model
             LIMIT ?1",
        )?;

        let trends = stmt
            .query_map(params![limit as i64], |row| {
                let total_calls: usize = row.get(3)?;
                let success_calls: usize = row.get(4)?;
                let success_rate = if total_calls > 0 {
                    (success_calls as f64 / total_calls as f64) * 100.0
                } else {
                    0.0
                };

                Ok(ModelTrendData {
                    model: row.get(0)?,
                    day: row.get(1)?,
                    session_count: row.get(2)?,
                    total_calls,
                    success_calls,
                    success_rate,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get model performance trends")?;

        Ok(trends)
    }

    pub fn get_stats_summary(&self) -> Result<MemoryStats> {
        let total_sessions: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;

        let total_messages: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;

        let total_tool_calls: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))?;

        let successful_tool_calls: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE success = 1",
            [],
            |row| row.get(0),
        )?;

        let total_tokens: u64 = self.conn.query_row(
            "SELECT COALESCE(SUM(tokens_used), 0) FROM messages WHERE tokens_used IS NOT NULL",
            [],
            |row| row.get(0),
        )?;

        let unique_models: usize =
            self.conn
                .query_row("SELECT COUNT(DISTINCT model) FROM sessions", [], |row| {
                    row.get(0)
                })?;

        let first_session: Option<DateTime<Utc>> = self
            .conn
            .query_row(
                "SELECT started_at FROM sessions ORDER BY started_at ASC LIMIT 1",
                [],
                |row| {
                    let s: String = row.get(0)?;
                    Ok(s.parse().unwrap_or_else(|_| Utc::now()))
                },
            )
            .ok();

        let latest_session: Option<DateTime<Utc>> = self
            .conn
            .query_row(
                "SELECT started_at FROM sessions ORDER BY started_at DESC LIMIT 1",
                [],
                |row| {
                    let s: String = row.get(0)?;
                    Ok(s.parse().unwrap_or_else(|_| Utc::now()))
                },
            )
            .ok();

        let tool_success_rate = if total_tool_calls > 0 {
            (successful_tool_calls as f64 / total_tool_calls as f64) * 100.0
        } else {
            0.0
        };

        Ok(MemoryStats {
            total_sessions,
            total_messages,
            total_tool_calls,
            successful_tool_calls,
            total_tokens,
            unique_models,
            first_session,
            latest_session,
            tool_success_rate,
        })
    }

    pub fn get_model_usage_stats(&self) -> Result<Vec<ModelUsageStats>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                s.model,
                COUNT(DISTINCT s.id) as session_count,
                COUNT(m.id) as message_count,
                COALESCE(SUM(m.tokens_used), 0) as total_tokens,
                COUNT(tc.id) as tool_call_count,
                SUM(CASE WHEN tc.success = 1 THEN 1 ELSE 0 END) as successful_tool_calls
             FROM sessions s
             LEFT JOIN messages m ON s.id = m.session_id
             LEFT JOIN tool_calls tc ON s.id = tc.session_id
             GROUP BY s.model
             ORDER BY session_count DESC",
        )?;

        let stats = stmt
            .query_map([], |row| {
                let tool_call_count: usize = row.get(4)?;
                let successful_tool_calls: usize = row.get(5)?;
                let tool_success_rate = if tool_call_count > 0 {
                    (successful_tool_calls as f64 / tool_call_count as f64) * 100.0
                } else {
                    0.0
                };

                Ok(ModelUsageStats {
                    model: row.get(0)?,
                    session_count: row.get(1)?,
                    message_count: row.get(2)?,
                    total_tokens: row.get(3)?,
                    tool_call_count,
                    tool_success_rate,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get model usage stats")?;

        Ok(stats)
    }

    pub fn get_tool_usage_stats(&self) -> Result<Vec<ToolUsageStats>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                tool_name,
                COUNT(*) as total_calls,
                SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful_calls,
                AVG(CASE WHEN success = 1 THEN 1.0 ELSE 0.0 END) * 100.0 as success_rate
             FROM tool_calls
             GROUP BY tool_name
             ORDER BY total_calls DESC",
        )?;

        let stats = stmt
            .query_map([], |row| {
                Ok(ToolUsageStats {
                    tool_name: row.get(0)?,
                    total_calls: row.get(1)?,
                    successful_calls: row.get(2)?,
                    success_rate: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get tool usage stats")?;

        Ok(stats)
    }

    pub fn get_daily_activity(&self, days: usize) -> Result<Vec<DailyActivity>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                date(started_at) as day,
                COUNT(DISTINCT id) as session_count,
                COUNT(DISTINCT model) as model_count
             FROM sessions
             WHERE started_at >= date('now', ?1)
             GROUP BY day
             ORDER BY day DESC",
        )?;

        let days_param = format!("-{} days", days);
        let activity = stmt
            .query_map(params![days_param], |row| {
                Ok(DailyActivity {
                    day: row.get(0)?,
                    session_count: row.get(1)?,
                    model_count: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get daily activity")?;

        Ok(activity)
    }

    pub fn save_performance_metric(
        &self,
        metric_type: &str,
        metric_name: &str,
        value_ms: u64,
        metadata: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO performance_metrics (metric_type, metric_name, value_ms, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                metric_type,
                metric_name,
                value_ms as i64,
                metadata,
                Utc::now().to_rfc3339()
            ],
        ).context("Failed to save performance metric")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_performance_metrics(
        &self,
        metric_type: &str,
        limit: usize,
    ) -> Result<Vec<PerformanceMetric>> {
        let mut stmt = self.conn.prepare(
            "SELECT metric_type, metric_name, value_ms, metadata, created_at
             FROM performance_metrics
             WHERE metric_type = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;

        let metrics = stmt
            .query_map(params![metric_type, limit as i64], |row| {
                let created_at: String = row.get(4)?;
                Ok(PerformanceMetric {
                    metric_type: row.get(0)?,
                    metric_name: row.get(1)?,
                    value_ms: row.get::<_, i64>(2)? as u64,
                    metadata: row.get(3)?,
                    created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get performance metrics")?;

        Ok(metrics)
    }

    pub fn get_performance_summary(&self) -> Result<PerformanceSummary> {
        let first_token_metrics: Vec<u64> = self.conn.prepare(
            "SELECT value_ms FROM performance_metrics WHERE metric_type = 'first_token' ORDER BY created_at DESC LIMIT 100"
        )?.query_map([], |row| {
            Ok(row.get::<_, i64>(0)? as u64)
        })?.collect::<Result<Vec<_>, _>>().unwrap_or_default();

        let total_latency_metrics: Vec<u64> = self.conn.prepare(
            "SELECT value_ms FROM performance_metrics WHERE metric_type = 'total_latency' ORDER BY created_at DESC LIMIT 100"
        )?.query_map([], |row| {
            Ok(row.get::<_, i64>(0)? as u64)
        })?.collect::<Result<Vec<_>, _>>().unwrap_or_default();

        let tool_metrics: Vec<u64> = self.conn.prepare(
            "SELECT value_ms FROM performance_metrics WHERE metric_type = 'tool_execution' ORDER BY created_at DESC LIMIT 100"
        )?.query_map([], |row| {
            Ok(row.get::<_, i64>(0)? as u64)
        })?.collect::<Result<Vec<_>, _>>().unwrap_or_default();

        let avg_first_token = if !first_token_metrics.is_empty() {
            first_token_metrics.iter().sum::<u64>() / first_token_metrics.len() as u64
        } else {
            0
        };

        let avg_total_latency = if !total_latency_metrics.is_empty() {
            total_latency_metrics.iter().sum::<u64>() / total_latency_metrics.len() as u64
        } else {
            0
        };

        let avg_tool = if !tool_metrics.is_empty() {
            tool_metrics.iter().sum::<u64>() / tool_metrics.len() as u64
        } else {
            0
        };

        let p95_first = calculate_p95(&first_token_metrics);
        let p95_tool = calculate_p95(&tool_metrics);

        Ok(PerformanceSummary {
            avg_first_token_ms: avg_first_token,
            avg_total_latency_ms: avg_total_latency,
            avg_tool_execution_ms: avg_tool,
            total_requests: first_token_metrics.len(),
            total_tools: tool_metrics.len(),
            p95_first_token_ms: p95_first,
            p95_tool_execution_ms: p95_tool,
        })
    }
}

fn calculate_p95(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64) * 0.95).ceil() as usize - 1;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn create_test_store() -> MemoryStore {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let db_path = format!(
            "/tmp/openshark_memory_test_{}_{}.db",
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

    fn create_test_tool_call(
        session_id: &str,
        id: &str,
        tool_name: &str,
        success: bool,
        result: &str,
    ) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            session_id: session_id.to_string(),
            tool_name: tool_name.to_string(),
            args: "{}".to_string(),
            result: result.to_string(),
            success,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_create_session() {
        let store = create_test_store();
        let result = store.create_session("sess-1", "model-a", "code");
        assert!(result.is_ok());

        let sessions = store.get_recent_sessions(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "sess-1");
        assert_eq!(sessions[0].model, "model-a");
        assert_eq!(sessions[0].task_type, "code");
        assert!(sessions[0].project_path.is_none());
    }

    #[test]
    fn test_create_session_with_project() {
        let store = create_test_store();
        let result =
            store.create_session_with_project("sess-1", "model-a", "code", "/home/user/project");
        assert!(result.is_ok());

        let sessions = store.get_sessions_by_project("/home/user/project").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "sess-1");
        assert_eq!(
            sessions[0].project_path.as_deref(),
            Some("/home/user/project")
        );
    }

    #[test]
    fn test_get_sessions_by_project() {
        let store = create_test_store();
        store
            .create_session_with_project("sess-1", "model-a", "code", "/project/a")
            .unwrap();
        store
            .create_session_with_project("sess-2", "model-b", "chat", "/project/a")
            .unwrap();
        store
            .create_session_with_project("sess-3", "model-c", "other", "/project/b")
            .unwrap();

        let sessions_a = store.get_sessions_by_project("/project/a").unwrap();
        assert_eq!(sessions_a.len(), 2);

        let sessions_b = store.get_sessions_by_project("/project/b").unwrap();
        assert_eq!(sessions_b.len(), 1);
        assert_eq!(sessions_b[0].id, "sess-3");

        let sessions_c = store.get_sessions_by_project("/nonexistent").unwrap();
        assert!(sessions_c.is_empty());
    }

    #[test]
    fn test_search_messages_by_project() {
        let store = create_test_store();
        store
            .create_session_with_project("sess-1", "model-a", "code", "/project/a")
            .unwrap();
        store
            .create_session_with_project("sess-2", "model-b", "chat", "/project/b")
            .unwrap();

        let msg1 = create_test_message("sess-1", "msg-1", "user", "rust programming in project a");
        let msg2 = create_test_message("sess-2", "msg-2", "user", "rust programming in project b");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let results_a = store
            .search_messages_by_project("rust", "/project/a", 10)
            .unwrap();
        assert_eq!(results_a.len(), 1);
        assert!(results_a[0].content.contains("project a"));

        let results_b = store
            .search_messages_by_project("rust", "/project/b", 10)
            .unwrap();
        assert_eq!(results_b.len(), 1);
        assert!(results_b[0].content.contains("project b"));
    }

    #[test]
    fn test_save_and_get_messages() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let msg1 = create_test_message("sess-1", "msg-1", "user", "Hello");
        let msg2 = create_test_message("sess-1", "msg-2", "assistant", "Hi there");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let messages = store.get_session_messages("sess-1").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].content, "Hi there");
    }

    #[test]
    fn test_save_and_search_tool_calls() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let tc1 = create_test_tool_call("sess-1", "tc-1", "fs", true, "ok");
        let tc2 = create_test_tool_call("sess-1", "tc-2", "terminal", false, "error");

        store.save_tool_call(&tc1).unwrap();
        store.save_tool_call(&tc2).unwrap();

        let calls = store.search_tool_calls_by_session("sess-1").unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].success);
        assert!(!calls[1].success);
    }

    #[test]
    fn test_search_messages() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let msg1 = create_test_message("sess-1", "msg-1", "user", "Hello world");
        let msg2 = create_test_message("sess-1", "msg-2", "user", "Goodbye world");
        let msg3 = create_test_message("sess-1", "msg-3", "user", "Rust programming");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();
        store.save_message(&msg3).unwrap();

        let results = store.search_messages("world", 10).unwrap();
        assert_eq!(results.len(), 2);

        let results = store.search_messages("Rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Rust programming");
    }

    #[test]
    fn test_get_recent_sessions() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();
        store.create_session("sess-2", "model-b", "chat").unwrap();
        store
            .create_session("sess-3", "model-c", "analysis")
            .unwrap();

        let sessions = store.get_recent_sessions(2).unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_save_and_get_analysis_results() {
        let store = create_test_store();
        store
            .save_analysis_result("model_performance", "model-a", "rate=0.95")
            .unwrap();
        store
            .save_analysis_result("model_performance", "model-b", "rate=0.80")
            .unwrap();

        let results = store.get_analysis_results("model_performance").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_get_all_tool_calls_with_sessions() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();
        store.create_session("sess-2", "model-b", "chat").unwrap();

        let tc1 = create_test_tool_call("sess-1", "tc-1", "fs", true, "ok");
        let tc2 = create_test_tool_call("sess-2", "tc-2", "terminal", false, "err");

        store.save_tool_call(&tc1).unwrap();
        store.save_tool_call(&tc2).unwrap();

        let results = store.get_all_tool_calls_with_sessions(10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_get_tool_failure_patterns() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();
        store.create_session("sess-2", "model-a", "code").unwrap();

        let tc1 = create_test_tool_call("sess-1", "tc-1", "fs", false, "err1");
        let tc2 = create_test_tool_call("sess-1", "tc-2", "terminal", false, "err2");
        let tc3 = create_test_tool_call("sess-2", "tc-3", "fs", false, "err1");
        let tc4 = create_test_tool_call("sess-2", "tc-4", "terminal", false, "err2");

        store.save_tool_call(&tc1).unwrap();
        store.save_tool_call(&tc2).unwrap();
        store.save_tool_call(&tc3).unwrap();
        store.save_tool_call(&tc4).unwrap();

        let patterns = store.get_tool_failure_patterns(10).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].0, "fs");
        assert_eq!(patterns[0].1, "terminal");
        assert_eq!(patterns[0].2, 2);
    }

    #[test]
    fn test_get_common_errors() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let tc1 = create_test_tool_call("sess-1", "tc-1", "fs", false, "permission denied");
        let tc2 = create_test_tool_call("sess-1", "tc-2", "fs", false, "permission denied");
        let tc3 = create_test_tool_call("sess-1", "tc-3", "terminal", false, "not found");

        store.save_tool_call(&tc1).unwrap();
        store.save_tool_call(&tc2).unwrap();
        store.save_tool_call(&tc3).unwrap();

        let errors = store.get_common_errors(10).unwrap();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].0.contains("permission denied"));
        assert_eq!(errors[0].1, 2);
    }

    #[test]
    fn test_get_session_quality_metrics() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let msg1 = create_test_message("sess-1", "msg-1", "user", "Hello");
        let tc1 = create_test_tool_call("sess-1", "tc-1", "fs", true, "ok");

        store.save_message(&msg1).unwrap();
        store.save_tool_call(&tc1).unwrap();

        let metrics = store.get_session_quality_metrics(10).unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].message_count, 1);
        assert_eq!(metrics[0].tool_call_count, 1);
        assert_eq!(metrics[0].tool_success_count, 1);
    }

    #[test]
    fn test_multiple_sessions_isolation() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();
        store.create_session("sess-2", "model-b", "chat").unwrap();

        let msg1 = create_test_message("sess-1", "msg-1", "user", "Hello");
        let msg2 = create_test_message("sess-2", "msg-2", "user", "World");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();

        let sess1_messages = store.get_session_messages("sess-1").unwrap();
        assert_eq!(sess1_messages.len(), 1);
        assert_eq!(sess1_messages[0].content, "Hello");

        let sess2_messages = store.get_session_messages("sess-2").unwrap();
        assert_eq!(sess2_messages.len(), 1);
        assert_eq!(sess2_messages[0].content, "World");
    }

    #[test]
    fn test_empty_search_returns_empty() {
        let store = create_test_store();
        let results = store.search_messages("nonexistent", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_analysis_results_update() {
        let store = create_test_store();
        store
            .save_analysis_result("test_cat", "key1", "value1")
            .unwrap();
        store
            .save_analysis_result("test_cat", "key1", "value2")
            .unwrap();

        let results = store.get_analysis_results("test_cat").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "value2");
    }

    #[test]
    fn test_semantic_search_basic() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let msg1 = create_test_message(
            "sess-1",
            "msg-1",
            "user",
            "How to write rust code for systems programming",
        );
        let msg2 =
            create_test_message("sess-1", "msg-2", "user", "Python scripting and automation");
        let msg3 = create_test_message(
            "sess-1",
            "msg-3",
            "user",
            "Rust memory management and ownership",
        );
        let msg4 = create_test_message("sess-1", "msg-4", "user", "Cooking recipes for beginners");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();
        store.save_message(&msg3).unwrap();
        store.save_message(&msg4).unwrap();

        let results = store.semantic_search("rust programming", 10).unwrap();
        assert!(!results.is_empty());

        let rust_count = results
            .iter()
            .filter(|(m, _)| m.content.contains("rust") || m.content.contains("Rust"))
            .count();
        assert!(rust_count >= 2, "Expected at least 2 rust-related results");
    }

    #[test]
    fn test_semantic_search_limit() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        for i in 0..5 {
            let msg = create_test_message(
                "sess-1",
                &format!("msg-{}", i),
                "user",
                &format!("topic {}", i),
            );
            store.save_message(&msg).unwrap();
        }

        let results = store.semantic_search("topic", 2).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_semantic_search_empty_query() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let msg = create_test_message("sess-1", "msg-1", "user", "Hello world");
        store.save_message(&msg).unwrap();

        let results = store.semantic_search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_semantic_search_no_results() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let msg = create_test_message("sess-1", "msg-1", "user", "Rust programming");
        store.save_message(&msg).unwrap();

        let results = store.semantic_search("quantum physics", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_semantic_search_scores_descending() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let msg1 = create_test_message("sess-1", "msg-1", "user", "rust programming language");
        let msg2 = create_test_message("sess-1", "msg-2", "user", "python scripting");
        let msg3 = create_test_message("sess-1", "msg-3", "user", "rust coding and development");

        store.save_message(&msg1).unwrap();
        store.save_message(&msg2).unwrap();
        store.save_message(&msg3).unwrap();

        let results = store.semantic_search("rust programming", 10).unwrap();
        assert!(results.len() >= 2);

        for i in 1..results.len() {
            assert!(
                results[i - 1].1 >= results[i].1,
                "Scores should be descending: {:?}",
                results.iter().map(|(_, s)| s).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_embedding_saved_with_message() {
        let store = create_test_store();
        store.create_session("sess-1", "model-a", "code").unwrap();

        let msg = create_test_message("sess-1", "msg-1", "user", "Hello world");
        store.save_message(&msg).unwrap();

        let mut stmt = store
            .conn
            .prepare("SELECT embedding_json FROM message_embeddings WHERE message_id = ?1")
            .unwrap();
        let embedding_json: String = stmt.query_row(params!["msg-1"], |row| row.get(0)).unwrap();
        let embedding: Vec<f32> = serde_json::from_str(&embedding_json).unwrap();
        assert_eq!(embedding.len(), crate::memory::embeddings::EMBEDDING_DIM);
    }
}

#[derive(Debug, Clone)]
pub struct SessionQualityMetrics {
    pub session_id: String,
    pub model: String,
    pub task_type: String,
    #[allow(dead_code)]
    pub started_at: DateTime<Utc>,
    pub message_count: usize,
    #[allow(dead_code)]
    pub tool_call_count: usize,
    #[allow(dead_code)]
    pub tool_success_count: usize,
    pub tool_success_rate: f64,
    pub quality_score: f64,
}

#[derive(Debug, Clone)]
pub struct ModelTrendData {
    pub model: String,
    pub day: String,
    pub session_count: usize,
    pub total_calls: usize,
    #[allow(dead_code)]
    pub success_calls: usize,
    pub success_rate: f64,
}

#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub total_sessions: usize,
    pub total_messages: usize,
    pub total_tool_calls: usize,
    pub successful_tool_calls: usize,
    pub total_tokens: u64,
    pub unique_models: usize,
    pub first_session: Option<DateTime<Utc>>,
    pub latest_session: Option<DateTime<Utc>>,
    pub tool_success_rate: f64,
}

#[derive(Debug, Clone)]
pub struct ModelUsageStats {
    pub model: String,
    pub session_count: usize,
    pub message_count: usize,
    pub total_tokens: u64,
    #[allow(dead_code)]
    pub tool_call_count: usize,
    pub tool_success_rate: f64,
}

#[derive(Debug, Clone)]
pub struct ToolUsageStats {
    pub tool_name: String,
    pub total_calls: usize,
    pub successful_calls: usize,
    pub success_rate: f64,
}

#[derive(Debug, Clone)]
pub struct DailyActivity {
    pub day: String,
    pub session_count: usize,
    pub model_count: usize,
}

#[derive(Debug, Clone)]
pub struct PerformanceMetric {
    #[allow(dead_code)]
    pub metric_type: String,
    #[allow(dead_code)]
    pub metric_name: String,
    #[allow(dead_code)]
    pub value_ms: u64,
    #[allow(dead_code)]
    pub metadata: Option<String>,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PerformanceSummary {
    pub avg_first_token_ms: u64,
    pub avg_total_latency_ms: u64,
    pub avg_tool_execution_ms: u64,
    pub total_requests: usize,
    pub total_tools: usize,
    pub p95_first_token_ms: u64,
    pub p95_tool_execution_ms: u64,
}
