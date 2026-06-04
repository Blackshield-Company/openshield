use super::Tool;
use crate::lsp::{LspClient, LspManager};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;

pub struct RefactorTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorRequest {
    pub operation: String,
    pub file_path: String,
    pub line: u32,
    pub character: u32,
    pub new_name: Option<String>,
    pub selection_start: Option<Position>,
    pub selection_end: Option<Position>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorResult {
    pub success: bool,
    pub operation: String,
    pub changes: Vec<FileChange>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub file_path: String,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub new_text: String,
}

impl Tool for RefactorTool {
    fn name(&self) -> &str {
        "refactor"
    }

    fn description(&self) -> &str {
        "LSP-based refactoring: extract_function, rename_symbol, inline_variable. Usage: refactor <extract_function|rename_symbol|inline_variable> <file> <line> <col> [new_name]"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.len() < 4 {
            return Ok(self.usage());
        }

        let operation = parts[0];
        let file_path = parts[1];
        let line: u32 = parts[2].parse().unwrap_or(0);
        let character: u32 = parts[3].parse().unwrap_or(0);
        let new_name = parts.get(4).map(|s| s.to_string());

        let request = RefactorRequest {
            operation: operation.to_string(),
            file_path: file_path.to_string(),
            line,
            character,
            new_name,
            selection_start: None,
            selection_end: None,
        };

        match operation {
            "extract_function" => self.extract_function(&request),
            "rename_symbol" => self.rename_symbol(&request),
            "inline_variable" => self.inline_variable(&request),
            _ => Ok(format!(
                "Unknown refactor operation: {}\n{}",
                operation,
                self.usage()
            )),
        }
    }
}

impl RefactorTool {
    fn usage(&self) -> String {
        "Refactor tool usage:\n\
         refactor extract_function <file> <line> <col>           - Extract selection to function\n\
         refactor rename_symbol <file> <line> <col> <new_name>  - Rename symbol\n\
         refactor inline_variable <file> <line> <col>        - Inline variable at position"
            .to_string()
    }

    fn extract_function(&self, request: &RefactorRequest) -> Result<String> {
        let (lsp_cmd, lsp_args, lang_id) = detect_lsp_server(&request.file_path);

        let client = LspClient::start(lsp_cmd, lsp_args, ".")?;

        let content = fs::read_to_string(&request.file_path)
            .with_context(|| format!("Failed to read {}", request.file_path))?;
        client.open_document(&request.file_path, lang_id, &content)?;

        let uri = format!(
            "file://{}",
            std::fs::canonicalize(&request.file_path)?.display()
        );

        let params = json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": request.line, "character": request.character },
                "end": { "line": request.line, "character": request.character }
            },
            "context": {
                "diagnostics": [],
                "only": ["refactor.extract.function"]
            }
        });

        let result = client.send_request_sync("textDocument/codeAction", params)?;

        let changes = parse_workspace_edit(&result, &request.file_path)?;

        let refactor_result = RefactorResult {
            success: !changes.is_empty(),
            operation: "extract_function".to_string(),
            changes,
            message: if result.is_null() {
                "No extract function action available at this position".to_string()
            } else {
                "Extract function refactoring computed".to_string()
            },
        };

        Ok(serde_json::to_string_pretty(&refactor_result)?)
    }

    fn rename_symbol(&self, request: &RefactorRequest) -> Result<String> {
        let new_name = request
            .new_name
            .as_ref()
            .context("rename_symbol requires a new_name argument")?;

        let (lsp_cmd, lsp_args, lang_id) = detect_lsp_server(&request.file_path);

        let client = LspClient::start(lsp_cmd, lsp_args, ".")?;

        let content = fs::read_to_string(&request.file_path)
            .with_context(|| format!("Failed to read {}", request.file_path))?;
        client.open_document(&request.file_path, lang_id, &content)?;

        let uri = format!(
            "file://{}",
            std::fs::canonicalize(&request.file_path)?.display()
        );

        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": request.line, "character": request.character },
            "newName": new_name
        });

        let result = client.send_request_sync("textDocument/rename", params)?;

        let changes = parse_workspace_edit(&result, &request.file_path)?;

        let refactor_result = RefactorResult {
            success: !changes.is_empty(),
            operation: "rename_symbol".to_string(),
            changes,
            message: format!("Renamed symbol to '{}'", new_name),
        };

        Ok(serde_json::to_string_pretty(&refactor_result)?)
    }

    fn inline_variable(&self, request: &RefactorRequest) -> Result<String> {
        let (lsp_cmd, lsp_args, lang_id) = detect_lsp_server(&request.file_path);

        let client = LspClient::start(lsp_cmd, lsp_args, ".")?;

        let content = fs::read_to_string(&request.file_path)
            .with_context(|| format!("Failed to read {}", request.file_path))?;
        client.open_document(&request.file_path, lang_id, &content)?;

        let uri = format!(
            "file://{}",
            std::fs::canonicalize(&request.file_path)?.display()
        );

        let params = json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": request.line, "character": request.character },
                "end": { "line": request.line, "character": request.character }
            },
            "context": {
                "diagnostics": [],
                "only": ["refactor.inline.variable"]
            }
        });

        let result = client.send_request_sync("textDocument/codeAction", params)?;

        let changes = parse_workspace_edit(&result, &request.file_path)?;

        let refactor_result = RefactorResult {
            success: !changes.is_empty(),
            operation: "inline_variable".to_string(),
            changes,
            message: if result.is_null() {
                "No inline variable action available at this position".to_string()
            } else {
                "Inline variable refactoring computed".to_string()
            },
        };

        Ok(serde_json::to_string_pretty(&refactor_result)?)
    }
}

fn detect_lsp_server(file_path: &str) -> (&'static str, &'static [&'static str], &'static str) {
    if file_path.ends_with(".rs") {
        ("rust-analyzer", &[], "rust")
    } else if file_path.ends_with(".py") {
        ("pylsp", &[], "python")
    } else if file_path.ends_with(".js") || file_path.ends_with(".ts") {
        ("typescript-language-server", &["--stdio"], "typescript")
    } else if file_path.ends_with(".go") {
        ("gopls", &[], "go")
    } else if file_path.ends_with(".c") || file_path.ends_with(".cpp") || file_path.ends_with(".h")
    {
        ("clangd", &[], "cpp")
    } else {
        ("rust-analyzer", &[], "rust")
    }
}

fn parse_workspace_edit(result: &Value, default_file: &str) -> Result<Vec<FileChange>> {
    let mut changes = Vec::new();

    if let Some(document_changes) = result.get("documentChanges").and_then(|v| v.as_array()) {
        for change in document_changes {
            if let Some(edits) = change.get("edits").and_then(|v| v.as_array()) {
                let file_path = change
                    .get("textDocument")
                    .and_then(|td| td.get("uri"))
                    .and_then(|u| u.as_str())
                    .unwrap_or(default_file)
                    .strip_prefix("file://")
                    .unwrap_or(default_file)
                    .to_string();

                let text_edits: Vec<TextEdit> = edits
                    .iter()
                    .filter_map(|edit| {
                        let range = edit.get("range")?;
                        let start = range.get("start")?;
                        let end = range.get("end")?;
                        let new_text = edit.get("newText")?.as_str()?;

                        Some(TextEdit {
                            start_line: start.get("line")?.as_u64()? as u32,
                            start_character: start.get("character")?.as_u64()? as u32,
                            end_line: end.get("line")?.as_u64()? as u32,
                            end_character: end.get("character")?.as_u64()? as u32,
                            new_text: new_text.to_string(),
                        })
                    })
                    .collect();

                if !text_edits.is_empty() {
                    changes.push(FileChange {
                        file_path,
                        edits: text_edits,
                    });
                }
            }
        }
    } else if let Some(changes_map) = result.get("changes").and_then(|v| v.as_object()) {
        for (uri, edits) in changes_map {
            let file_path = uri.strip_prefix("file://").unwrap_or(uri).to_string();

            let text_edits: Vec<TextEdit> = edits
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .filter_map(|edit| {
                    let range = edit.get("range")?;
                    let start = range.get("start")?;
                    let end = range.get("end")?;
                    let new_text = edit.get("newText")?.as_str()?;

                    Some(TextEdit {
                        start_line: start.get("line")?.as_u64()? as u32,
                        start_character: start.get("character")?.as_u64()? as u32,
                        end_line: end.get("line")?.as_u64()? as u32,
                        end_character: end.get("character")?.as_u64()? as u32,
                        new_text: new_text.to_string(),
                    })
                })
                .collect();

            if !text_edits.is_empty() {
                changes.push(FileChange {
                    file_path,
                    edits: text_edits,
                });
            }
        }
    }

    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_refactor_request_parsing() {
        let request = RefactorRequest {
            operation: "rename_symbol".to_string(),
            file_path: "/tmp/test.rs".to_string(),
            line: 10,
            character: 5,
            new_name: Some("new_name".to_string()),
            selection_start: None,
            selection_end: None,
        };

        assert_eq!(request.operation, "rename_symbol");
        assert_eq!(request.line, 10);
        assert_eq!(request.character, 5);
    }

    #[test]
    fn test_detect_lsp_server_rust() {
        let (cmd, args, lang) = detect_lsp_server("test.rs");
        assert_eq!(cmd, "rust-analyzer");
        assert_eq!(lang, "rust");
        assert!(args.is_empty());
    }

    #[test]
    fn test_detect_lsp_server_python() {
        let (cmd, args, lang) = detect_lsp_server("test.py");
        assert_eq!(cmd, "pylsp");
        assert_eq!(lang, "python");
        assert!(args.is_empty());
    }

    #[test]
    fn test_detect_lsp_server_typescript() {
        let (cmd, args, lang) = detect_lsp_server("test.ts");
        assert_eq!(cmd, "typescript-language-server");
        assert_eq!(lang, "typescript");
        assert_eq!(args, &["--stdio"]);
    }

    #[test]
    fn test_detect_lsp_server_go() {
        let (cmd, args, lang) = detect_lsp_server("test.go");
        assert_eq!(cmd, "gopls");
        assert_eq!(lang, "go");
        assert!(args.is_empty());
    }

    #[test]
    fn test_detect_lsp_server_cpp() {
        let (cmd, args, lang) = detect_lsp_server("test.cpp");
        assert_eq!(cmd, "clangd");
        assert_eq!(lang, "cpp");
        assert!(args.is_empty());
    }

    #[test]
    fn test_detect_lsp_server_default() {
        let (cmd, _, lang) = detect_lsp_server("test.unknown");
        assert_eq!(cmd, "rust-analyzer");
        assert_eq!(lang, "rust");
    }

    #[test]
    fn test_parse_workspace_edit_empty() {
        let result = json!({});
        let changes = parse_workspace_edit(&result, "/tmp/test.rs").unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_parse_workspace_edit_with_changes() {
        let result = json!({
            "changes": {
                "file:///tmp/test.rs": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 5 }
                        },
                        "newText": "new_text"
                    }
                ]
            }
        });

        let changes = parse_workspace_edit(&result, "/tmp/test.rs").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].file_path, "/tmp/test.rs");
        assert_eq!(changes[0].edits.len(), 1);
        assert_eq!(changes[0].edits[0].new_text, "new_text");
        assert_eq!(changes[0].edits[0].start_line, 0);
        assert_eq!(changes[0].edits[0].start_character, 0);
        assert_eq!(changes[0].edits[0].end_line, 0);
        assert_eq!(changes[0].edits[0].end_character, 5);
    }

    #[test]
    fn test_parse_workspace_edit_document_changes() {
        let result = json!({
            "documentChanges": [
                {
                    "textDocument": { "uri": "file:///tmp/test.rs" },
                    "edits": [
                        {
                            "range": {
                                "start": { "line": 1, "character": 2 },
                                "end": { "line": 1, "character": 10 }
                            },
                            "newText": "replacement"
                        }
                    ]
                }
            ]
        });

        let changes = parse_workspace_edit(&result, "/tmp/test.rs").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].edits[0].new_text, "replacement");
    }

    #[test]
    fn test_refactor_result_serialization() {
        let result = RefactorResult {
            success: true,
            operation: "rename_symbol".to_string(),
            changes: vec![FileChange {
                file_path: "/tmp/test.rs".to_string(),
                edits: vec![TextEdit {
                    start_line: 0,
                    start_character: 0,
                    end_line: 0,
                    end_character: 5,
                    new_text: "new_name".to_string(),
                }],
            }],
            message: "Renamed successfully".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("rename_symbol"));
        assert!(json.contains("new_name"));
    }

    #[test]
    fn test_refactor_tool_empty_args() {
        let tool = RefactorTool;
        let result = tool.execute("").unwrap();
        assert!(result.contains("Refactor tool usage"));
    }

    #[test]
    fn test_refactor_tool_unknown_operation() {
        let tool = RefactorTool;
        let result = tool.execute("unknown /tmp/test.rs 0 0").unwrap();
        assert!(result.contains("Unknown refactor operation"));
    }
}

// ---------------------------------------------------------------------------
// Async variant using persistent LspManager connections
// ---------------------------------------------------------------------------

use async_trait::async_trait;
use super::AsyncTool;

pub struct RefactorAsyncTool {
    manager: std::sync::Arc<LspManager>,
}

impl RefactorAsyncTool {
    pub fn new(manager: std::sync::Arc<LspManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl AsyncTool for RefactorAsyncTool {
    fn name(&self) -> &str { "refactor" }
    fn description(&self) -> &str {
        "LSP-based refactoring: extract_function, rename_symbol, inline_variable. Usage: refactor <extract_function|rename_symbol|inline_variable> <file> <line> <col> [new_name]"
    }

    async fn execute_async(&self, args: &str) -> anyhow::Result<String> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.len() < 4 {
            return Ok("Refactor tool usage:\n\
                 refactor extract_function <file> <line> <col>           - Extract selection to function\n\
                 refactor rename_symbol <file> <line> <col> <new_name>  - Rename symbol\n\
                 refactor inline_variable <file> <line> <col>        - Inline variable at position"
                .to_string());
        }

        let operation = parts[0];
        let file_path = parts[1];
        let line: u32 = parts[2].parse().unwrap_or(0);
        let character: u32 = parts[3].parse().unwrap_or(0);
        let new_name = parts.get(4).map(|s| s.to_string());

        let (lsp_cmd, lsp_args, lang_id) = LspManager::detect_server(file_path);
        let server = self.manager.get_or_create_server(lang_id, lsp_cmd, lsp_args).await?;

        let content = std::fs::read_to_string(file_path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", file_path, e))?;
        server.ensure_document_open(file_path, &content).await?;

        let uri = format!("file://{}", std::fs::canonicalize(file_path)?.display());

        match operation {
            "extract_function" => {
                let params = serde_json::json!({
                    "textDocument": { "uri": uri },
                    "range": { "start": { "line": line, "character": character }, "end": { "line": line, "character": character } },
                    "context": { "diagnostics": [], "only": ["refactor.extract.function"] }
                });
                let result = server.request("textDocument/codeAction", params).await?;
                let changes = parse_workspace_edit_async(&result, file_path)?;
                let refactor_result = RefactorResult {
                    success: !changes.is_empty(),
                    operation: "extract_function".to_string(),
                    changes,
                    message: if result.is_null() { "No extract function action available".to_string() } else { "Extract function computed".to_string() },
                };
                Ok(serde_json::to_string_pretty(&refactor_result)?)
            }
            "rename_symbol" => {
                let new_name = new_name.ok_or_else(|| anyhow::anyhow!("rename_symbol requires new_name"))?;
                let params = serde_json::json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                    "newName": new_name
                });
                let result = server.request("textDocument/rename", params).await?;
                let changes = parse_workspace_edit_async(&result, file_path)?;
                let msg = format!("Renamed symbol to '{}'", new_name);
                let refactor_result = RefactorResult {
                    success: !changes.is_empty(),
                    operation: "rename_symbol".to_string(),
                    changes,
                    message: msg,
                };
                Ok(serde_json::to_string_pretty(&refactor_result)?)
            }
            "inline_variable" => {
                let params = serde_json::json!({
                    "textDocument": { "uri": uri },
                    "range": { "start": { "line": line, "character": character }, "end": { "line": line, "character": character } },
                    "context": { "diagnostics": [], "only": ["refactor.inline.variable"] }
                });
                let result = server.request("textDocument/codeAction", params).await?;
                let changes = parse_workspace_edit_async(&result, file_path)?;
                let refactor_result = RefactorResult {
                    success: !changes.is_empty(),
                    operation: "inline_variable".to_string(),
                    changes,
                    message: if result.is_null() { "No inline variable action available".to_string() } else { "Inline variable computed".to_string() },
                };
                Ok(serde_json::to_string_pretty(&refactor_result)?)
            }
            _ => Ok(format!("Unknown refactor operation: {}", operation)),
        }
    }
}

fn parse_workspace_edit_async(result: &serde_json::Value, default_file: &str) -> anyhow::Result<Vec<FileChange>> {
    let mut changes = Vec::new();
    if let Some(document_changes) = result.get("documentChanges").and_then(|v| v.as_array()) {
        for change in document_changes {
            if let Some(edits) = change.get("edits").and_then(|v| v.as_array()) {
                let fp = change.get("textDocument").and_then(|td| td.get("uri")).and_then(|u| u.as_str()).unwrap_or(default_file).strip_prefix("file://").unwrap_or(default_file).to_string();
                let text_edits: Vec<TextEdit> = edits.iter().filter_map(|edit| {
                    let range = edit.get("range")?;
                    let start = range.get("start")?;
                    let end = range.get("end")?;
                    let new_text = edit.get("newText")?.as_str()?;
                    Some(TextEdit { start_line: start.get("line")?.as_u64()? as u32, start_character: start.get("character")?.as_u64()? as u32, end_line: end.get("line")?.as_u64()? as u32, end_character: end.get("character")?.as_u64()? as u32, new_text: new_text.to_string() })
                }).collect();
                if !text_edits.is_empty() { changes.push(FileChange { file_path: fp, edits: text_edits }); }
            }
        }
    } else if let Some(changes_map) = result.get("changes").and_then(|v| v.as_object()) {
        for (uri, edits) in changes_map {
            let fp = uri.strip_prefix("file://").unwrap_or(uri).to_string();
            let text_edits: Vec<TextEdit> = edits.as_array().unwrap_or(&Vec::new()).iter().filter_map(|edit| {
                let range = edit.get("range")?;
                let start = range.get("start")?;
                let end = range.get("end")?;
                let new_text = edit.get("newText")?.as_str()?;
                Some(TextEdit { start_line: start.get("line")?.as_u64()? as u32, start_character: start.get("character")?.as_u64()? as u32, end_line: end.get("line")?.as_u64()? as u32, end_character: end.get("character")?.as_u64()? as u32, new_text: new_text.to_string() })
            }).collect();
            if !text_edits.is_empty() { changes.push(FileChange { file_path: fp, edits: text_edits }); }
        }
    }
    Ok(changes)
}
