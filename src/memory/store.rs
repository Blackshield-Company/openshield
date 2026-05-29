use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub model: String,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
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
    pub created_at: DateTime<Utc>,
}

pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    pub fn new(db_path: &Path) -> Result<Self> {
        std::fs::create_dir_all(db_path.parent().unwrap_or(Path::new(".")))
            .context("Failed to create memory directory")?;
        
        let conn = Connection::open(db_path)
            .context("Failed to open memory database")?;
        
        let store = Self { conn };
        store.init_tables()?;
        Ok(store)
    }
    
    fn init_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                started_at TEXT NOT NULL,
                model TEXT NOT NULL,
                task_type TEXT NOT NULL DEFAULT 'general'
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
            
            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_session ON tool_calls(session_id);
            "
        ).context("Failed to create tables")?;
        
        Ok(())
    }
    
    pub fn create_session(&self, id: &str, model: &str, task_type: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, started_at, model, task_type) VALUES (?1, ?2, ?3, ?4)",
            params![id, Utc::now().to_rfc3339(), model, task_type],
        ).context("Failed to create session")?;
        Ok(())
    }
    
    pub fn save_message(&self, msg: &Message) -> Result<()> {
        self.conn.execute(
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
        ).context("Failed to save message")?;
        Ok(())
    }
    
    pub fn get_session_messages(&self,
        session_id: &str,
    ) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created_at, tokens_used 
             FROM messages 
             WHERE session_id = ?1 
             ORDER BY created_at"
        )?;
        
        let messages = stmt.query_map(params![session_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get::<_, String>(4)?.parse().unwrap_or_else(|_| Utc::now()),
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
    
    pub fn search_messages(&self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let pattern = format!("%{query}%");
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created_at, tokens_used 
             FROM messages 
             WHERE content LIKE ?1 
             ORDER BY created_at DESC 
             LIMIT ?2"
        )?;
        
        let messages = stmt.query_map(params![pattern, limit as i64], |row| {
            Ok(Message {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get::<_, String>(4)?.parse().unwrap_or_else(|_| Utc::now()),
                tokens_used: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to search messages")?;
        
        Ok(messages)
    }
    
    pub fn get_recent_sessions(&self,
        limit: usize,
    ) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, model, task_type 
             FROM sessions 
             ORDER BY started_at DESC 
             LIMIT ?1"
        )?;
        
        let sessions = stmt.query_map(params![limit as i64], |row| {
            Ok(Session {
                id: row.get(0)?,
                started_at: row.get::<_, String>(1)?.parse().unwrap_or_else(|_| Utc::now()),
                model: row.get(2)?,
                task_type: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to get sessions")?;
        
        Ok(sessions)
    }
}
