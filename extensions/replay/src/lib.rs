//! Built-in session replay module.
//!
//! Lists, opens, and exports recorded sessions from `memory.db` or a
//! SessionExport JSON file. `export --redact` writes a share pack with
//! keys, PEM material, `~/.ssh` paths, and an optional denylist removed.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use openshield_sdk::{BuiltinModule, ModuleHooks, ModuleHost};
use serde::Serialize;

mod format;
mod redact;
mod store;

pub use redact::{RedactionReport, contains_unredacted_secret, redact_json, redact_text};
pub use store::{
    SessionListItem, SharePack, TimelineEvent, default_export_path, leftover_secrets,
    load_pack_file, load_session, open_db, write_pack,
};

/// Clap subcommands for `openshield replay`.
#[derive(Debug, Clone, Subcommand)]
pub enum ReplayCli {
    /// Show whether the replay module is compiled and enabled.
    Status,
    /// List recorded sessions in the memory database.
    List {
        /// Maximum number of sessions to show.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
        /// Optional path to a `memory.db` (defaults to the host path).
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Print a stepable text timeline for a session, export file, or db.
    Open {
        /// Session id, export JSON path, or a `memory.db` path.
        target: Option<String>,
        /// Print JSON instead of a text timeline.
        #[arg(long)]
        json: bool,
        /// Optional path to a `memory.db` when `target` is a session id.
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Write a session to a JSON share pack.
    Export {
        /// Session id to export.
        session_id: String,
        /// Scrub keys, PEM material, `~/.ssh` paths, and `--deny` strings.
        #[arg(long)]
        redact: bool,
        /// Output path (defaults under the sessions directory).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Extra literal strings to redact (repeatable).
        #[arg(long = "deny", value_name = "TEXT")]
        denylist: Vec<String>,
        /// Optional path to a `memory.db`.
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Validate an export file; for redacted packs, fail if secrets remain.
    Verify {
        /// Path to a session export / share pack.
        path: PathBuf,
    },
}

/// JSON/text report for `openshield replay status` and `GET /api/v1/replay/status`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReplayStatus {
    pub id: &'static str,
    pub compiled: bool,
    pub enabled: bool,
}

impl ReplayStatus {
    /// Status for this compiled crate, given the runtime switch.
    pub fn current(enabled: bool) -> Self {
        Self {
            id: "replay",
            compiled: true,
            enabled,
        }
    }
}

/// The replay built-in module.
pub struct ReplayModule;

impl BuiltinModule for ReplayModule {
    fn id(&self) -> &'static str {
        "replay"
    }

    fn hooks(&self) -> ModuleHooks {
        ModuleHooks::default()
    }
}

/// What `run_cli` wants the host to print (or how to exit).
#[derive(Debug, Clone)]
pub enum CliOutput {
    /// `replay status`.
    Status(ReplayStatus),
    /// Plain text for a terminal.
    Text(String),
    /// JSON for `--json` or machine-readable commands.
    Json(serde_json::Value),
    /// The module is compiled but disabled; the host should exit 2.
    Disabled,
}

/// Run a `openshield replay` subcommand.
///
/// `status` always succeeds, even when the module is disabled. Other
/// commands return [`CliOutput::Disabled`] so the host can exit
/// [`DISABLED_EXIT`].
pub fn run_cli(cmd: ReplayCli, host: &ModuleHost) -> anyhow::Result<CliOutput> {
    if !matches!(cmd, ReplayCli::Status) && !host.replay_enabled() {
        return Ok(CliOutput::Disabled);
    }
    match cmd {
        ReplayCli::Status => Ok(CliOutput::Status(ReplayStatus::current(
            host.replay_enabled(),
        ))),
        ReplayCli::List { limit, json, db } => cmd_list(host, limit, json, db.as_deref()),
        ReplayCli::Open { target, json, db } => {
            cmd_open(host, target.as_deref(), json, db.as_deref())
        }
        ReplayCli::Export {
            session_id,
            redact,
            output,
            denylist,
            db,
        } => cmd_export(
            host,
            &session_id,
            redact,
            output.as_deref(),
            &denylist,
            db.as_deref(),
        ),
        ReplayCli::Verify { path } => cmd_verify(host, &path),
    }
}

fn resolve_db<'a>(host: &'a ModuleHost, db: Option<&'a Path>) -> &'a Path {
    db.unwrap_or(host.memory_db_path.as_path())
}

fn cmd_list(
    host: &ModuleHost,
    limit: usize,
    json: bool,
    db: Option<&Path>,
) -> anyhow::Result<CliOutput> {
    let conn = open_db(resolve_db(host, db))?;
    let items = store::list_sessions(&conn, host, limit)?;
    if json {
        Ok(CliOutput::Json(serde_json::to_value(&items)?))
    } else {
        Ok(CliOutput::Text(format::format_list(&items)))
    }
}

fn cmd_open(
    host: &ModuleHost,
    target: Option<&str>,
    json: bool,
    db: Option<&Path>,
) -> anyhow::Result<CliOutput> {
    match target {
        None => {
            let conn = open_db(resolve_db(host, db))?;
            let items = store::list_sessions(&conn, host, 20)?;
            let mut text = String::from(
                "pass a session id, export JSON path, or memory.db to open a timeline\n\n",
            );
            text.push_str(&format::format_list(&items));
            if json {
                Ok(CliOutput::Json(serde_json::to_value(&items)?))
            } else {
                Ok(CliOutput::Text(text))
            }
        }
        Some(target) => {
            let path = Path::new(target);
            if path.is_file() {
                if store::looks_like_sqlite(path) {
                    let conn = open_db(path)?;
                    let items = store::list_sessions(&conn, host, 100)?;
                    if items.len() == 1 {
                        let pack = store::load_session(&conn, host, &items[0].id)?
                            .ok_or_else(|| anyhow::anyhow!("session {} not found", items[0].id))?;
                        return output_pack(&pack, json);
                    }
                    let mut text = format!(
                        "database {} has {} sessions; pass a session id to open one\n\n",
                        path.display(),
                        items.len()
                    );
                    text.push_str(&format::format_list(&items));
                    if json {
                        return Ok(CliOutput::Json(serde_json::to_value(&items)?));
                    }
                    return Ok(CliOutput::Text(text));
                }
                let pack = load_pack_file(path, host)?;
                return output_pack(&pack, json);
            }
            let conn = open_db(resolve_db(host, db))?;
            let pack = load_session(&conn, host, target)?
                .ok_or_else(|| anyhow::anyhow!("session {target} not found"))?;
            output_pack(&pack, json)
        }
    }
}

fn output_pack(pack: &SharePack, json: bool) -> anyhow::Result<CliOutput> {
    if json {
        Ok(CliOutput::Json(serde_json::to_value(pack)?))
    } else {
        Ok(CliOutput::Text(format::format_timeline(pack)))
    }
}

fn cmd_export(
    host: &ModuleHost,
    session_id: &str,
    redact: bool,
    output: Option<&Path>,
    denylist: &[String],
    db: Option<&Path>,
) -> anyhow::Result<CliOutput> {
    let conn = open_db(resolve_db(host, db))?;
    let mut pack = load_session(&conn, host, session_id)?
        .ok_or_else(|| anyhow::anyhow!("session {session_id} not found"))?;
    if redact {
        pack.redact(denylist)?;
    }
    let path = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_export_path(session_id, redact));
    write_pack(&path, &pack)?;
    Ok(CliOutput::Text(format!(
        "wrote {}{}",
        path.display(),
        if redact { " (redacted share pack)" } else { "" }
    )))
}

fn cmd_verify(host: &ModuleHost, path: &Path) -> anyhow::Result<CliOutput> {
    let pack = load_pack_file(path, host)?;
    let redacted = pack.redaction.as_ref().map(|r| r.redacted).unwrap_or(false);
    if redacted {
        let leftover = leftover_secrets(&pack)?;
        if leftover > 0 {
            anyhow::bail!(
                "verify failed: {leftover} unredacted secret pattern(s) remain in {}",
                path.display()
            );
        }
        let n = pack.redaction.as_ref().map(|r| r.replacements).unwrap_or(0);
        Ok(CliOutput::Text(format!(
            "ok: redacted share pack, {n} replacement(s)"
        )))
    } else {
        Ok(CliOutput::Text(
            "ok: export valid (not marked as redacted)".to_string(),
        ))
    }
}

/// Print a status report as plain text.
pub fn format_status(status: &ReplayStatus) -> String {
    format!(
        "module: {}\ncompiled: {}\nenabled: {}",
        status.id, status.compiled, status.enabled
    )
}

/// Render a [`CliOutput`] other than [`CliOutput::Disabled`].
pub fn format_output(output: &CliOutput) -> anyhow::Result<String> {
    match output {
        CliOutput::Status(status) => Ok(format_status(status)),
        CliOutput::Text(text) => Ok(text.clone()),
        CliOutput::Json(value) => Ok(serde_json::to_string_pretty(value)?),
        CliOutput::Disabled => {
            Ok("replay module is disabled ([modules.replay] enabled = false)".to_string())
        }
    }
}

/// Directory where session export files are written
/// (`$XDG_DATA_HOME/openshield/sessions`, or `./openshield/sessions`).
pub fn sessions_dir() -> PathBuf {
    dirs_data_home().join("openshield").join("sessions")
}

fn dirs_data_home() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local").join("share");
    }
    PathBuf::from(".")
}

/// Estimate USD cost from a single token total. Input and output rates are
/// blended 50/50 because the session does not record the split.
pub fn estimate_cost_usd(tokens_used: u64, cost_per_1k_input: f64, cost_per_1k_output: f64) -> f64 {
    (tokens_used as f64 / 1000.0) * (cost_per_1k_input + cost_per_1k_output) / 2.0
}

/// Exit code used when a module is compiled but disabled at runtime.
pub const DISABLED_EXIT: u8 = 2;

#[cfg(feature = "web-api")]
mod routes {
    use axum::Json;
    use axum::Router;
    use axum::extract::{Path, Query, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use openshield_sdk::ModuleHost;
    use serde::Deserialize;
    use serde_json::json;

    use crate::ReplayStatus;
    use crate::store::{self, default_export_path, write_pack};

    /// Routes mounted at `/api/v1/replay`.
    ///
    /// Generic over the host app state so this sub-router can be nested
    /// into `Router<AppState>` via [`axum::extract::FromRef`].
    pub fn router<S>() -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
        ModuleHost: axum::extract::FromRef<S>,
    {
        Router::new()
            .route("/status", get(status))
            .route("/sessions", get(list_sessions))
            .route("/sessions/{id}", get(get_session))
            .route("/sessions/{id}/export", post(export_session))
    }

    async fn status(State(host): State<ModuleHost>) -> Json<ReplayStatus> {
        Json(ReplayStatus::current(host.replay_enabled()))
    }

    #[derive(Debug, Deserialize)]
    struct ListQuery {
        #[serde(default = "default_limit")]
        limit: usize,
    }

    fn default_limit() -> usize {
        20
    }

    async fn list_sessions(
        State(host): State<ModuleHost>,
        Query(query): Query<ListQuery>,
    ) -> Response {
        match (|| {
            let conn = store::open_db(&host.memory_db_path)?;
            store::list_sessions(&conn, &host, query.limit.clamp(1, 500))
        })() {
            Ok(sessions) => Json(json!({ "sessions": sessions })).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response(),
        }
    }

    async fn get_session(State(host): State<ModuleHost>, Path(id): Path<String>) -> Response {
        match (|| {
            let conn = store::open_db(&host.memory_db_path)?;
            store::load_session(&conn, &host, &id)
        })() {
            Ok(Some(pack)) => Json(pack).into_response(),
            Ok(None) => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("session {id} not found") })),
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response(),
        }
    }

    #[derive(Debug, Deserialize)]
    struct ExportBody {
        #[serde(default)]
        redact: bool,
        #[serde(default)]
        denylist: Vec<String>,
        /// Also write the pack under the sessions directory.
        #[serde(default)]
        save: bool,
    }

    async fn export_session(
        State(host): State<ModuleHost>,
        Path(id): Path<String>,
        body: Option<Json<ExportBody>>,
    ) -> Response {
        let body = body.map(|j| j.0).unwrap_or(ExportBody {
            redact: false,
            denylist: Vec::new(),
            save: false,
        });
        match export_inner(&host, &id, body) {
            Ok(value) => Json(value).into_response(),
            Err(ExportError::NotFound(msg)) => {
                (StatusCode::NOT_FOUND, Json(json!({ "error": msg }))).into_response()
            }
            Err(ExportError::Other(e)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response(),
        }
    }

    enum ExportError {
        NotFound(String),
        Other(String),
    }

    fn export_inner(
        host: &ModuleHost,
        id: &str,
        body: ExportBody,
    ) -> Result<serde_json::Value, ExportError> {
        let conn =
            store::open_db(&host.memory_db_path).map_err(|e| ExportError::Other(e.to_string()))?;
        let mut pack = store::load_session(&conn, host, id)
            .map_err(|e| ExportError::Other(e.to_string()))?
            .ok_or_else(|| ExportError::NotFound(format!("session {id} not found")))?;
        if body.redact {
            pack.redact(&body.denylist)
                .map_err(|e| ExportError::Other(e.to_string()))?;
        }
        let mut path = None;
        if body.save {
            let p = default_export_path(id, body.redact);
            write_pack(&p, &pack).map_err(|e| ExportError::Other(e.to_string()))?;
            path = Some(p.to_string_lossy().into_owned());
        }
        Ok(json!({ "pack": pack, "path": path }))
    }
}

#[cfg(feature = "web-api")]
pub use routes::router;

#[cfg(test)]
mod tests {
    use super::*;
    use openshield_sdk::{ModuleToggle, ModulesConfig};
    use rusqlite::Connection;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn host(enabled: bool) -> ModuleHost {
        ModuleHost::new(
            PathBuf::from("/tmp/memory.db"),
            ModulesConfig {
                replay: ModuleToggle { enabled },
                ..Default::default()
            },
            None,
        )
        .with_model_cost_rates(vec![("k3".into(), 0.001, 0.003)])
    }

    fn temp_db() -> (PathBuf, Connection) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = PathBuf::from(format!(
            "/tmp/openshield_replay_test_{}_{}.db",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                started_at TEXT NOT NULL,
                model TEXT NOT NULL,
                task_type TEXT NOT NULL DEFAULT 'general',
                project_path TEXT,
                archived INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                tokens_used INTEGER
            );
            CREATE TABLE tool_calls (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                args TEXT NOT NULL,
                result TEXT NOT NULL,
                success INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE approvals (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                tool TEXT NOT NULL,
                args TEXT NOT NULL,
                risk TEXT NOT NULL,
                reason TEXT NOT NULL,
                decision TEXT NOT NULL,
                created_at TEXT NOT NULL,
                resolved_at TEXT NOT NULL
            );
            CREATE TABLE audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT,
                timestamp TEXT NOT NULL,
                action TEXT NOT NULL,
                tool TEXT NOT NULL,
                args TEXT NOT NULL,
                approved INTEGER NOT NULL,
                risk_level TEXT NOT NULL,
                reason TEXT NOT NULL
            );",
        )
        .unwrap();
        (path, conn)
    }

    fn seed(conn: &Connection) {
        conn.execute(
            "INSERT INTO sessions (id, started_at, model, archived)
             VALUES ('sess-1', '2026-11-26T07:00:00+00:00', 'k3', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, session_id, role, content, created_at, tokens_used)
             VALUES ('m1', 'sess-1', 'user', 'please use key sk-abcdefghijklmnopqrstuvwxyz0123456789AB and ~/.ssh/id_ed25519', '2026-11-26T07:00:01+00:00', 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, session_id, role, content, created_at, tokens_used)
             VALUES ('m2', 'sess-1', 'assistant', 'ok', '2026-11-26T07:00:02+00:00', 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tool_calls (id, session_id, tool_name, args, result, success, created_at)
             VALUES ('t1', 'sess-1', 'fs', '{\"path\":\".\"}', 'ok', 1, '2026-11-26T07:00:03+00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO approvals (id, session_id, tool, args, risk, reason, decision, created_at, resolved_at)
             VALUES ('a1', 'sess-1', 'terminal', '{}', 'high', 'destructive', 'approved', '2026-11-26T07:00:04+00:00', '2026-11-26T07:00:05+00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO audit_log (session_id, timestamp, action, tool, args, approved, risk_level, reason)
             VALUES ('sess-1', '2026-11-26T07:00:06+00:00', 'tool_execution', 'fs', '{}', 1, 'low', 'approved')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn status_reports_runtime_switch() {
        let on = run_cli(ReplayCli::Status, &host(true)).unwrap();
        match on {
            CliOutput::Status(s) => {
                assert!(s.compiled);
                assert!(s.enabled);
            }
            other => panic!("expected status, got {other:?}"),
        }
        let off = run_cli(ReplayCli::Status, &host(false)).unwrap();
        match off {
            CliOutput::Status(s) => {
                assert!(!s.enabled);
                assert!(format_status(&s).contains("enabled: false"));
            }
            other => panic!("expected status, got {other:?}"),
        }
    }

    #[test]
    fn disabled_module_blocks_list() {
        let out = run_cli(
            ReplayCli::List {
                limit: 20,
                json: false,
                db: None,
            },
            &host(false),
        )
        .unwrap();
        assert!(matches!(out, CliOutput::Disabled));
    }

    #[test]
    fn module_id_is_replay() {
        assert_eq!(ReplayModule.id(), "replay");
    }

    #[test]
    fn list_open_export_redact_and_verify() {
        let (db_path, conn) = temp_db();
        seed(&conn);
        drop(conn);

        let mut h = host(true);
        h.memory_db_path = db_path.clone();

        let listed = run_cli(
            ReplayCli::List {
                limit: 10,
                json: true,
                db: Some(db_path.clone()),
            },
            &h,
        )
        .unwrap();
        let CliOutput::Json(value) = listed else {
            panic!("expected json list");
        };
        assert_eq!(value[0]["id"], "sess-1");
        assert_eq!(value[0]["tokens_used"], 2000);
        // 2000 tokens * ((0.001+0.003)/2) / 1000 = 0.004
        assert!((value[0]["estimated_cost_usd"].as_f64().unwrap() - 0.004).abs() < 1e-9);

        let opened = run_cli(
            ReplayCli::Open {
                target: Some("sess-1".into()),
                json: false,
                db: Some(db_path.clone()),
            },
            &h,
        )
        .unwrap();
        let CliOutput::Text(text) = opened else {
            panic!("expected text timeline");
        };
        assert!(text.contains("session: sess-1"));
        assert!(text.contains("message user"));
        assert!(text.contains("tool fs ok"));
        assert!(text.contains("approval approved"));
        assert!(text.contains("audit allowed"));
        assert!(text.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789AB"));

        let out_path = PathBuf::from(format!(
            "/tmp/openshield_replay_pack_{}_{}.json",
            std::process::id(),
            db_path.file_stem().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_file(&out_path);
        let exported = run_cli(
            ReplayCli::Export {
                session_id: "sess-1".into(),
                redact: true,
                output: Some(out_path.clone()),
                denylist: vec!["destructive".into()],
                db: Some(db_path.clone()),
            },
            &h,
        )
        .unwrap();
        let CliOutput::Text(msg) = exported else {
            panic!("expected export path text");
        };
        assert!(msg.contains("redacted share pack"));
        assert!(out_path.exists());

        let pack = load_pack_file(&out_path, &h).unwrap();
        assert!(pack.redaction.as_ref().unwrap().redacted);
        let json = serde_json::to_string(&pack).unwrap();
        assert!(!json.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789AB"));
        assert!(json.contains("sk-***"));
        assert!(!json.contains("~/.ssh/id_ed25519"));
        assert!(json.contains("[REDACTED_SSH_PATH]"));
        assert!(!json.contains("destructive"));
        assert!(leftover_secrets(&pack).unwrap() == 0);

        let verified = run_cli(
            ReplayCli::Verify {
                path: out_path.clone(),
            },
            &h,
        )
        .unwrap();
        let CliOutput::Text(ok) = verified else {
            panic!("expected verify text");
        };
        assert!(ok.starts_with("ok: redacted share pack"));

        // Opening the pack itself should work.
        let from_file = run_cli(
            ReplayCli::Open {
                target: Some(out_path.to_string_lossy().into_owned()),
                json: true,
                db: None,
            },
            &h,
        )
        .unwrap();
        let CliOutput::Json(file_pack) = from_file else {
            panic!("expected json pack");
        };
        assert_eq!(file_pack["session_id"], "sess-1");
        assert!(!file_pack["timeline"].as_array().unwrap().is_empty());
    }

    #[test]
    fn estimate_blends_input_and_output_rates() {
        assert_eq!(estimate_cost_usd(1000, 1.0, 3.0), 2.0);
        assert_eq!(estimate_cost_usd(0, 1.0, 3.0), 0.0);
    }
}
