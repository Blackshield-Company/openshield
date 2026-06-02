use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
}

impl SessionExport {
    /// Export current session state to a JSON file.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize session export")?;
        std::fs::write(path, json)
            .context("Failed to write session export file")?;
        Ok(())
    }

    /// Load a session export from a JSON file.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .context("Failed to read session export file")?;
        let export: SessionExport = serde_json::from_str(&json)
            .context("Failed to parse session export JSON")?;
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
            },
        }
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

/// Export a session to the default location (~/.local/share/openshark/sessions/).
pub fn export_to_default(export: &SessionExport) -> Result<std::path::PathBuf> {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openshark")
        .join("sessions");
    std::fs::create_dir_all(&dir)
        .context("Failed to create sessions directory")?;

    let filename = format!(
        "openshark_session_{}_{}.json",
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
        .join("openshark")
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
    exports.sort_by(|a, b| b.1.exported_at.cmp(&a.1.exported_at));
    Ok(exports)
}
