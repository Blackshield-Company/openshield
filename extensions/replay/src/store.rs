//! Read sessions from `memory.db` and from SessionExport JSON files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use openshield_sdk::ModuleHost;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::redact::{RedactionReport, redact_json};
use crate::{estimate_cost_usd, sessions_dir};

/// One row in `openshield replay list` / `GET /api/v1/replay/sessions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionListItem {
    pub id: String,
    pub started_at: String,
    pub model: String,
    pub message_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub tokens_used: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    pub cost_is_estimate: bool,
}

/// A shareable / openable session pack. Field names stay compatible with
/// the host `SessionExport` JSON so a file dropped on `replay open` works.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharePack {
    pub version: String,
    pub exported_at: DateTime<Utc>,
    pub session_id: String,
    pub model: String,
    #[serde(default)]
    pub messages: Vec<PackMessage>,
    #[serde(default)]
    pub branches: Vec<serde_json::Value>,
    #[serde(default)]
    pub metadata: PackMetadata,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<PackToolCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<PackApproval>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audit: Vec<PackAudit>,
    /// Filled only when the pack was produced with `--redact`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction: Option<RedactionReport>,
    /// Merged timeline for API/CLI display. Omitted from older files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline: Vec<TimelineEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackMetadata {
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub tool_calls_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cost_is_estimate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_used: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackToolCall {
    pub id: String,
    #[serde(default)]
    pub session_id: String,
    pub tool_name: String,
    pub args: String,
    pub result: String,
    pub success: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackApproval {
    pub id: String,
    #[serde(default)]
    pub session_id: String,
    pub tool: String,
    pub args: String,
    pub risk: String,
    pub reason: String,
    pub decision: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackAudit {
    #[serde(default)]
    pub id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub tool: String,
    #[serde(default)]
    pub args: String,
    pub approved: bool,
    pub risk_level: String,
    pub reason: String,
}

/// Ordered event used by `replay open` and `GET /sessions/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimelineEvent {
    Message {
        timestamp: DateTime<Utc>,
        role: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tokens_used: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        estimated_cost_usd: Option<f64>,
    },
    ToolCall {
        timestamp: DateTime<Utc>,
        id: String,
        tool_name: String,
        args: String,
        result: String,
        success: bool,
    },
    Approval {
        timestamp: DateTime<Utc>,
        id: String,
        tool: String,
        args: String,
        risk: String,
        reason: String,
        decision: String,
    },
    Audit {
        timestamp: DateTime<Utc>,
        tool: String,
        approved: bool,
        risk_level: String,
        reason: String,
    },
}

impl TimelineEvent {
    fn timestamp(&self) -> DateTime<Utc> {
        match self {
            TimelineEvent::Message { timestamp, .. }
            | TimelineEvent::ToolCall { timestamp, .. }
            | TimelineEvent::Approval { timestamp, .. }
            | TimelineEvent::Audit { timestamp, .. } => *timestamp,
        }
    }

    fn order_key(&self) -> u8 {
        match self {
            TimelineEvent::Message { .. } => 0,
            TimelineEvent::ToolCall { .. } => 1,
            TimelineEvent::Approval { .. } => 2,
            TimelineEvent::Audit { .. } => 3,
        }
    }
}

impl SharePack {
    /// Rebuild the merged timeline from the packed collections.
    pub fn rebuild_timeline(&mut self, rates: Option<(f64, f64)>) {
        let mut events = Vec::new();
        for m in &self.messages {
            let cost = match (m.tokens_used, rates) {
                (Some(tok), Some((inp, out))) => Some(estimate_cost_usd(tok as u64, inp, out)),
                _ => None,
            };
            events.push(TimelineEvent::Message {
                timestamp: m.timestamp,
                role: m.role.clone(),
                content: m.content.clone(),
                tokens_used: m.tokens_used,
                estimated_cost_usd: cost,
            });
        }
        for t in &self.tool_calls {
            events.push(TimelineEvent::ToolCall {
                timestamp: t.created_at,
                id: t.id.clone(),
                tool_name: t.tool_name.clone(),
                args: t.args.clone(),
                result: t.result.clone(),
                success: t.success,
            });
        }
        for a in &self.approvals {
            events.push(TimelineEvent::Approval {
                timestamp: a.resolved_at,
                id: a.id.clone(),
                tool: a.tool.clone(),
                args: a.args.clone(),
                risk: a.risk.clone(),
                reason: a.reason.clone(),
                decision: a.decision.clone(),
            });
        }
        for a in &self.audit {
            events.push(TimelineEvent::Audit {
                timestamp: a.timestamp,
                tool: a.tool.clone(),
                approved: a.approved,
                risk_level: a.risk_level.clone(),
                reason: a.reason.clone(),
            });
        }
        events.sort_by(|a, b| {
            a.timestamp()
                .cmp(&b.timestamp())
                .then(a.order_key().cmp(&b.order_key()))
        });
        self.timeline = events;
    }

    /// Apply redaction to every string in this pack and attach the report.
    pub fn redact(&mut self, denylist: &[String]) -> Result<()> {
        let value = serde_json::to_value(&self)?;
        let (redacted, report) = redact_json(&value, denylist);
        let mut pack: SharePack = serde_json::from_value(redacted)?;
        pack.redaction = Some(report);
        *self = pack;
        Ok(())
    }
}

/// Open the host memory database, or another sqlite file, read-only.
pub fn open_db(path: &Path) -> Result<Connection> {
    if !path.exists() {
        bail!("memory database not found at {}", path.display());
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open {}", path.display()))
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![name],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

fn parse_dt(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .map(|d| d.with_timezone(&Utc))
        .or_else(|_| raw.parse::<DateTime<Utc>>())
        .unwrap_or_else(|_| Utc::now())
}

/// List sessions from a database, annotating cost from `host` rates.
pub fn list_sessions(
    conn: &Connection,
    host: &ModuleHost,
    limit: usize,
) -> Result<Vec<SessionListItem>> {
    if !table_exists(conn, "sessions") {
        return Ok(Vec::new());
    }
    let has_archived = conn
        .prepare("PRAGMA table_info(sessions)")
        .ok()
        .and_then(|mut s| {
            let names = s
                .query_map([], |row| row.get::<_, String>(1))
                .ok()?
                .filter_map(|n| n.ok())
                .collect::<Vec<_>>();
            Some(names.iter().any(|n| n == "archived"))
        })
        .unwrap_or(false);
    let sql = if has_archived {
        "SELECT s.id, s.started_at, s.model,
                (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id),
                (SELECT substr(m2.content, 1, 80) FROM messages m2
                  WHERE m2.session_id = s.id AND m2.role = 'user'
                  ORDER BY m2.created_at ASC LIMIT 1),
                (SELECT COALESCE(SUM(m3.tokens_used), 0) FROM messages m3
                  WHERE m3.session_id = s.id)
         FROM sessions s
         WHERE s.archived = 0
         ORDER BY s.started_at DESC
         LIMIT ?1"
    } else {
        "SELECT s.id, s.started_at, s.model,
                (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id),
                (SELECT substr(m2.content, 1, 80) FROM messages m2
                  WHERE m2.session_id = s.id AND m2.role = 'user'
                  ORDER BY m2.created_at ASC LIMIT 1),
                (SELECT COALESCE(SUM(m3.tokens_used), 0) FROM messages m3
                  WHERE m3.session_id = s.id)
         FROM sessions s
         ORDER BY s.started_at DESC
         LIMIT ?1"
    };
    // SUM(tokens_used) fails on very old DBs without the column — fall back.
    let sql_no_tokens = if has_archived {
        "SELECT s.id, s.started_at, s.model,
                (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id),
                (SELECT substr(m2.content, 1, 80) FROM messages m2
                  WHERE m2.session_id = s.id AND m2.role = 'user'
                  ORDER BY m2.created_at ASC LIMIT 1),
                0
         FROM sessions s
         WHERE s.archived = 0
         ORDER BY s.started_at DESC
         LIMIT ?1"
    } else {
        "SELECT s.id, s.started_at, s.model,
                (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id),
                (SELECT substr(m2.content, 1, 80) FROM messages m2
                  WHERE m2.session_id = s.id AND m2.role = 'user'
                  ORDER BY m2.created_at ASC LIMIT 1),
                0
         FROM sessions s
         ORDER BY s.started_at DESC
         LIMIT ?1"
    };

    let mut stmt = conn.prepare(sql).or_else(|_| conn.prepare(sql_no_tokens))?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            let id: String = row.get(0)?;
            let started_at: String = row.get(1)?;
            let model: String = row.get(2)?;
            let message_count: i64 = row.get(3)?;
            let title: Option<String> = row.get(4)?;
            let tokens_used: i64 = row.get::<_, i64>(5).unwrap_or(0);
            Ok((
                id,
                started_at,
                model,
                message_count,
                title,
                tokens_used as u64,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to list sessions")?;

    Ok(rows
        .into_iter()
        .map(
            |(id, started_at, model, message_count, title, tokens_used)| {
                let (estimated_cost_usd, cost_is_estimate) = match host.cost_rates_for(&model) {
                    Some((inp, out)) if tokens_used > 0 => {
                        (Some(estimate_cost_usd(tokens_used, inp, out)), true)
                    }
                    _ => (None, false),
                };
                SessionListItem {
                    id,
                    started_at,
                    model,
                    message_count,
                    title,
                    tokens_used,
                    estimated_cost_usd,
                    cost_is_estimate,
                }
            },
        )
        .collect())
}

/// Load one session from a database as a share pack.
pub fn load_session(conn: &Connection, host: &ModuleHost, id: &str) -> Result<Option<SharePack>> {
    if !table_exists(conn, "sessions") {
        return Ok(None);
    }
    let row = conn
        .query_row(
            "SELECT id, started_at, model FROM sessions WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .context("failed to load session")?;
    let Some((session_id, started_at, model)) = row else {
        return Ok(None);
    };

    let mut messages = Vec::new();
    if table_exists(conn, "messages") {
        let sql_tokens = "SELECT role, content, created_at, tokens_used
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC";
        let sql_plain = "SELECT role, content, created_at
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC";
        if let Ok(mut stmt) = conn.prepare(sql_tokens) {
            let rows = stmt.query_map(params![id], |row| {
                let tokens: Option<i64> = row.get::<_, Option<i64>>(3).unwrap_or(None);
                Ok(PackMessage {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    images: None,
                    reasoning: None,
                    timestamp: parse_dt(&row.get::<_, String>(2)?),
                    tokens_used: tokens.map(|t| t as u32),
                })
            })?;
            for r in rows {
                messages.push(r?);
            }
        } else {
            let mut stmt = conn.prepare(sql_plain)?;
            let rows = stmt.query_map(params![id], |row| {
                Ok(PackMessage {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    images: None,
                    reasoning: None,
                    timestamp: parse_dt(&row.get::<_, String>(2)?),
                    tokens_used: None,
                })
            })?;
            for r in rows {
                messages.push(r?);
            }
        }
    }

    let mut tool_calls = Vec::new();
    if table_exists(conn, "tool_calls") {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, tool_name, args, result, success, created_at
             FROM tool_calls WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            Ok(PackToolCall {
                id: row.get(0)?,
                session_id: row.get(1)?,
                tool_name: row.get(2)?,
                args: row.get(3)?,
                result: row.get(4)?,
                success: row.get::<_, i64>(5)? != 0,
                created_at: parse_dt(&row.get::<_, String>(6)?),
            })
        })?;
        for r in rows {
            tool_calls.push(r?);
        }
    }

    let mut approvals = Vec::new();
    if table_exists(conn, "approvals") {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, tool, args, risk, reason, decision, created_at, resolved_at
             FROM approvals WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            Ok(PackApproval {
                id: row.get(0)?,
                session_id: row.get(1)?,
                tool: row.get(2)?,
                args: row.get(3)?,
                risk: row.get(4)?,
                reason: row.get(5)?,
                decision: row.get(6)?,
                created_at: parse_dt(&row.get::<_, String>(7)?),
                resolved_at: parse_dt(&row.get::<_, String>(8)?),
            })
        })?;
        for r in rows {
            approvals.push(r?);
        }
    }

    let mut audit = Vec::new();
    if table_exists(conn, "audit_log") {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, timestamp, action, tool, args, approved, risk_level, reason
             FROM audit_log WHERE session_id = ?1 ORDER BY timestamp ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            Ok(PackAudit {
                id: row.get(0)?,
                session_id: row.get(1)?,
                timestamp: parse_dt(&row.get::<_, String>(2)?),
                action: row.get(3)?,
                tool: row.get(4)?,
                args: row.get(5)?,
                approved: row.get::<_, i64>(6)? != 0,
                risk_level: row.get(7)?,
                reason: row.get(8)?,
            })
        })?;
        for r in rows {
            audit.push(r?);
        }
    }

    let tokens_used: u64 = messages
        .iter()
        .map(|m| m.tokens_used.unwrap_or(0) as u64)
        .sum();
    let rates = host.cost_rates_for(&model);
    let (estimated_cost_usd, cost_is_estimate) = match rates {
        Some((inp, out)) if tokens_used > 0 => {
            (Some(estimate_cost_usd(tokens_used, inp, out)), true)
        }
        _ => (None, false),
    };

    let mut pack = SharePack {
        version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: parse_dt(&started_at),
        session_id,
        model,
        metadata: PackMetadata {
            tokens_used,
            tool_calls_count: tool_calls.len(),
            summary: None,
            estimated_cost_usd,
            cost_is_estimate,
        },
        messages,
        branches: Vec::new(),
        tool_calls,
        approvals,
        audit,
        redaction: None,
        timeline: Vec::new(),
    };
    pack.rebuild_timeline(rates);
    Ok(Some(pack))
}

/// Load a previously written export / share pack from disk.
pub fn load_pack_file(path: &Path, host: &ModuleHost) -> Result<SharePack> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut pack: SharePack = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse session export {}", path.display()))?;
    if pack.timeline.is_empty() {
        pack.rebuild_timeline(host.cost_rates_for(&pack.model));
    }
    if pack.metadata.estimated_cost_usd.is_none()
        && pack.metadata.tokens_used > 0
        && let Some((inp, out)) = host.cost_rates_for(&pack.model)
    {
        pack.metadata.estimated_cost_usd =
            Some(estimate_cost_usd(pack.metadata.tokens_used, inp, out));
        pack.metadata.cost_is_estimate = true;
    }
    Ok(pack)
}

/// True if `path` looks like a SQLite database (extension or file magic).
pub fn looks_like_sqlite(path: &Path) -> bool {
    if matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("db") | Some("sqlite") | Some("sqlite3")
    ) {
        return true;
    }
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 16];
        if f.read(&mut buf).ok() == Some(16) {
            return buf.starts_with(b"SQLite format 3");
        }
    }
    false
}

/// Default output path for a share pack.
pub fn default_export_path(session_id: &str, redacted: bool) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%d_%H%M%S");
    let name = if redacted {
        format!("openshield_session_{session_id}_{stamp}_redacted.json")
    } else {
        format!("openshield_session_{session_id}_{stamp}.json")
    };
    sessions_dir().join(name)
}

/// Write a pack as pretty JSON, creating the parent directory if needed.
pub fn write_pack(path: &Path, pack: &SharePack) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(pack).context("failed to serialize share pack")?;
    std::fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Scan a pack for leftover secret material. Returns the number of hits.
pub fn leftover_secrets(pack: &SharePack) -> Result<usize> {
    let value = serde_json::to_value(pack)?;
    Ok(count_secrets(&value))
}

fn count_secrets(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::String(s) => {
            if crate::redact::contains_unredacted_secret(s) {
                1
            } else {
                0
            }
        }
        serde_json::Value::Array(items) => items.iter().map(count_secrets).sum(),
        serde_json::Value::Object(map) => map
            .iter()
            .filter(|(k, _)| k.as_str() != "redaction")
            .map(|(_, v)| count_secrets(v))
            .sum(),
        _ => 0,
    }
}
