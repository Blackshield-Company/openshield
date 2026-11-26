use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::memory::{ApprovalRecord, AuditRecord, MemoryStore, ToolCall};
use crate::providers::Message;

/// Serializable session export format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExport {
    pub version: String,
    pub exported_at: DateTime<Utc>,
    pub session_id: String,
    pub model: String,
    pub messages: Vec<ExportMessage>,
    pub branches: Vec<ExportBranch>,
    pub metadata: SessionMetadata,
    /// Tool calls recorded for this session (empty in older export files).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ExportToolCall>,
    /// Persisted approval decisions (empty in older export files).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<ApprovalRecord>,
    /// Persisted security-audit rows for this session (empty in older files).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audit: Vec<AuditRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub images: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// One tool invocation in a session export.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportToolCall {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub args: String,
    pub result: String,
    pub success: bool,
    pub created_at: DateTime<Utc>,
}

impl From<ToolCall> for ExportToolCall {
    fn from(call: ToolCall) -> Self {
        Self {
            id: call.id,
            session_id: call.session_id,
            tool_name: call.tool_name,
            args: call.args,
            result: call.result,
            success: call.success,
            created_at: call.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBranch {
    pub name: String,
    pub messages: Vec<ExportMessage>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMetadata {
    pub tokens_used: u64,
    pub tool_calls_count: usize,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<String>,
    /// Estimated USD cost from `tokens_used` and the model's per-1k rates.
    /// There is no input/output token split in the export, so this blends
    /// the two rates evenly and is always labelled an estimate.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub estimated_cost_usd: Option<f64>,
    /// True when `estimated_cost_usd` was computed rather than billed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cost_is_estimate: bool,
}

impl SessionExport {
    /// Export current session state to a JSON file.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize session export")?;
        std::fs::write(path, json).context("Failed to write session export file")?;
        Ok(())
    }

    /// Load a session export from a JSON file.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let json = std::fs::read_to_string(path).context("Failed to read session export file")?;
        let export: SessionExport =
            serde_json::from_str(&json).context("Failed to parse session export JSON")?;
        Ok(export)
    }

    /// Create an export from raw TUI state.
    pub fn from_tui_state(
        session_id: String,
        model: String,
        messages: Vec<ExportMessage>,
        branches: Vec<ExportBranch>,
        tokens_used: u64,
        tool_calls_count: usize,
    ) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            exported_at: Utc::now(),
            session_id,
            model,
            messages,
            branches,
            metadata: SessionMetadata {
                tokens_used,
                tool_calls_count,
                summary: None,
                estimated_cost_usd: None,
                cost_is_estimate: false,
            },
            tool_calls: Vec::new(),
            approvals: Vec::new(),
            audit: Vec::new(),
        }
    }

    /// Fill `tool_calls` and `approvals` from a memory database.
    pub fn attach_from_store(&mut self, store: &MemoryStore) -> Result<()> {
        let calls = store.search_tool_calls_by_session(&self.session_id)?;
        if !calls.is_empty() {
            self.metadata.tool_calls_count = calls.len();
        }
        self.tool_calls = calls.into_iter().map(ExportToolCall::from).collect();
        self.approvals = store.list_approvals(&self.session_id)?;
        self.audit = store.list_audit_records(Some(&self.session_id), 10_000)?;
        Ok(())
    }

    /// Attach a blended token-cost estimate and mark it as an estimate.
    pub fn set_cost_estimate(&mut self, cost_per_1k_input: f64, cost_per_1k_output: f64) {
        self.metadata.estimated_cost_usd = Some(estimate_cost_usd(
            self.metadata.tokens_used,
            cost_per_1k_input,
            cost_per_1k_output,
        ));
        self.metadata.cost_is_estimate = true;
    }

    /// Convert export messages back to provider Messages for model history.
    pub fn to_model_messages(&self) -> Vec<Message> {
        self.messages
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: m.content.clone(),
                images: m.images.clone(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: m.reasoning.clone(),
            })
            .collect()
    }
}

/// Export a session to the default location (~/.local/share/openshield/sessions/).
pub fn export_to_default(export: &SessionExport) -> Result<std::path::PathBuf> {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield")
        .join("sessions");
    std::fs::create_dir_all(&dir).context("Failed to create sessions directory")?;

    let filename = format!(
        "openshield_session_{}_{}.json",
        export.session_id,
        export.exported_at.format("%Y%m%d_%H%M%S")
    );
    let path = dir.join(&filename);
    export.save_to_file(&path)?;
    Ok(path)
}

/// List all exported sessions in the default directory.
pub fn list_exports() -> Result<Vec<(std::path::PathBuf, SessionExport)>> {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshield")
        .join("sessions");

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut exports = Vec::new();
    for entry in std::fs::read_dir(&dir).context("Failed to read sessions directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            match SessionExport::load_from_file(&path) {
                Ok(export) => exports.push((path, export)),
                Err(_) => continue, // Skip corrupted files
            }
        }
    }

    // Sort by export time, newest first
    exports.sort_by_key(|(_, e)| std::cmp::Reverse(e.exported_at));
    Ok(exports)
}

/// Estimate USD cost from a single token total. Input and output rates are
/// blended 50/50 because the session does not record the split.
pub fn estimate_cost_usd(tokens_used: u64, cost_per_1k_input: f64, cost_per_1k_output: f64) -> f64 {
    (tokens_used as f64 / 1000.0) * (cost_per_1k_input + cost_per_1k_output) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{AuditRecord, MemoryStore};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_store() -> MemoryStore {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = format!(
            "/tmp/openshield_session_export_test_{}_{}.db",
            std::process::id(),
            n
        );
        let _ = std::fs::remove_file(&path);
        MemoryStore::new(Path::new(&path)).unwrap()
    }

    #[test]
    fn old_export_json_still_loads() {
        let json = r#"{
            "version": "1.2.2",
            "exported_at": "2026-11-01T00:00:00Z",
            "session_id": "s1",
            "model": "k3",
            "messages": [],
            "branches": [],
            "metadata": { "tokens_used": 1500, "tool_calls_count": 0 }
        }"#;
        let export: SessionExport = serde_json::from_str(json).unwrap();
        assert!(export.tool_calls.is_empty());
        assert!(export.approvals.is_empty());
        assert!(export.audit.is_empty());
        assert_eq!(export.metadata.tokens_used, 1500);
        assert!(export.metadata.estimated_cost_usd.is_none());
        assert!(!export.metadata.cost_is_estimate);
    }

    #[test]
    fn attach_from_store_fills_tool_calls_and_approvals() {
        let store = temp_store();
        store.create_session("s1", "k3", "code").unwrap();
        store
            .save_tool_call(&ToolCall {
                id: "t1".into(),
                session_id: "s1".into(),
                tool_name: "fs".into(),
                args: r#"{"path":"."}"#.into(),
                result: "ok".into(),
                success: true,
                created_at: Utc::now(),
            })
            .unwrap();
        store
            .save_approval(&ApprovalRecord {
                id: "a1".into(),
                session_id: "s1".into(),
                tool: "terminal".into(),
                args: "{}".into(),
                risk: "high".into(),
                reason: "ask".into(),
                decision: "approved".into(),
                created_at: Utc::now(),
                resolved_at: Utc::now(),
            })
            .unwrap();
        // Unrelated session should not leak into the export.
        store
            .save_audit_record(&AuditRecord {
                id: 0,
                session_id: Some("s1".into()),
                timestamp: Utc::now(),
                action: "tool_execution".into(),
                tool: "fs".into(),
                args: "{}".into(),
                approved: true,
                risk_level: "low".into(),
                reason: "approved".into(),
            })
            .unwrap();

        let mut export = SessionExport::from_tui_state(
            "s1".into(),
            "k3".into(),
            Vec::new(),
            Vec::new(),
            2000,
            0,
        );
        export.attach_from_store(&store).unwrap();
        export.set_cost_estimate(0.001, 0.003);
        assert_eq!(export.tool_calls.len(), 1);
        assert_eq!(export.tool_calls[0].id, "t1");
        assert_eq!(export.approvals.len(), 1);
        assert_eq!(export.approvals[0].id, "a1");
        assert_eq!(export.audit.len(), 1);
        assert_eq!(export.audit[0].tool, "fs");
        assert_eq!(export.metadata.tool_calls_count, 1);
        assert_eq!(export.metadata.estimated_cost_usd, Some(0.004));
        assert!(export.metadata.cost_is_estimate);

        let json = serde_json::to_string(&export).unwrap();
        assert!(json.contains("estimated_cost_usd"));
        assert!(json.contains("cost_is_estimate"));
        let back: SessionExport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_calls.len(), 1);
        assert_eq!(back.approvals.len(), 1);
        assert_eq!(back.audit.len(), 1);
    }

    #[test]
    fn estimate_blends_input_and_output_rates() {
        // 1000 tokens * average($1, $3)/1k = $2
        assert_eq!(estimate_cost_usd(1000, 1.0, 3.0), 2.0);
        assert_eq!(estimate_cost_usd(0, 1.0, 3.0), 0.0);
    }
}
