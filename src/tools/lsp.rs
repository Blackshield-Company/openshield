use super::Tool;
use crate::lsp::{LspClient, Symbol};
use anyhow::{Context, Result};
use std::fs;

pub struct LspTool;

impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "LSP symbol operations: symbols, definition, hover. Usage: lsp <symbols|def|hover> <file> [line] [col]"
    }

    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(4, ' ').collect();
        if parts.len() < 2 {
            return Ok(self.usage());
        }

        let cmd = parts[0];
        let file_path = parts[1];

        // Detect language and LSP server
        let (lsp_cmd, lsp_args, lang_id) = detect_lsp_server(file_path);

        let client = LspClient::start(lsp_cmd, lsp_args, ".")?;

        // Open the document
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path))?;
        client.open_document(file_path, lang_id, &content)?;

        let result = match cmd {
            "symbols" => {
                let symbols = client.document_symbols(file_path)?;
                format_symbols(&symbols)
            }
            "def" | "definition" => {
                if parts.len() < 4 {
                    return Ok("Usage: lsp def <file> <line> <col>".to_string());
                }
                let line: u32 = parts[2].parse().unwrap_or(0);
                let col: u32 = parts[3].parse().unwrap_or(0);
                let defs = client.goto_definition(file_path, line, col)?;
                format_symbols(&defs)
            }
            "hover" => {
                if parts.len() < 4 {
                    return Ok("Usage: lsp hover <file> <line> <col>".to_string());
                }
                let line: u32 = parts[2].parse().unwrap_or(0);
                let col: u32 = parts[3].parse().unwrap_or(0);
                match client.hover(file_path, line, col)? {
                    Some(text) => text,
                    None => "No hover information found.".to_string(),
                }
            }
            _ => format!("Unknown lsp command: {}\n{}", cmd, self.usage()),
        };

        // Note: We can't easily shut down the client here due to borrow constraints
        // In production, we'd use a persistent LSP connection
        Ok(result)
    }
}

impl LspTool {
    fn usage(&self) -> String {
        "LSP tool usage:\n\
         lsp symbols <file>          - List document symbols\n\
         lsp def <file> <line> <col> - Go to definition\n\
         lsp hover <file> <line> <col> - Show type info"
            .to_string()
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
        ("rust-analyzer", &[], "rust") // default
    }
}

fn format_symbols(symbols: &[Symbol]) -> String {
    if symbols.is_empty() {
        return "No symbols found.".to_string();
    }

    let mut lines = Vec::new();
    for s in symbols {
        let detail = s
            .detail
            .as_ref()
            .map(|d| format!(" ({})", d))
            .unwrap_or_default();
        lines.push(format!(
            "{}:{}:{} | {}{} | {}",
            s.file, s.line, s.character, s.name, detail, s.kind
        ));
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Async variant using persistent LspManager connections
// ---------------------------------------------------------------------------

use async_trait::async_trait;
use super::AsyncTool;
use crate::lsp::LspManager;
use std::sync::Arc;

pub struct LspAsyncTool {
    manager: Arc<LspManager>,
}

impl LspAsyncTool {
    pub fn new(manager: Arc<LspManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl AsyncTool for LspAsyncTool {
    fn name(&self) -> &str { "lsp" }
    fn description(&self) -> &str {
        "LSP symbol operations: symbols, definition, hover, diagnostics. Usage: lsp <symbols|def|hover|diagnostics> <file> [line] [col]"
    }

    async fn execute_async(&self, args: &str) -> anyhow::Result<String> {
        let parts: Vec<&str> = args.splitn(4, ' ').collect();
        if parts.len() < 2 {
            return Ok("LSP tool usage:\n\
                 lsp symbols <file>             - List document symbols\n\
                 lsp def <file> <line> <col>    - Go to definition\n\
                 lsp hover <file> <line> <col>  - Show type info\n\
                 lsp diagnostics <file>         - Get diagnostics for a file\n\
                 lsp diagnostics /all           - Get all diagnostics"
                .to_string());
        }

        let cmd = parts[0];
        let file_path = parts[1];

        // Special case: diagnostics /all doesn't need a server
        if cmd == "diagnostics" && file_path == "/all" {
            let store = self.manager.diagnostics_store();
            let all = store.get_all().await;
            if all.is_empty() {
                return Ok("No diagnostics available.".to_string());
            }
            let mut lines = Vec::new();
            for (uri, diags) in &all {
                lines.push(format!("--- {} ({} issues) ---", uri, diags.len()));
                for d in diags {
                    lines.push(format!("  [{}] {}:{}:{} - {}", d.severity, d.file, d.line, d.character, d.message));
                }
            }
            return Ok(lines.join("\n"));
        }

        if cmd == "diagnostics" {
            let store = self.manager.diagnostics_store();
            let uri = format!("file://{}", std::fs::canonicalize(file_path)?.display());
            let diags = store.get(&uri).await;
            if diags.is_empty() {
                return Ok(format!("No diagnostics for {}.", file_path));
            }
            let lines: Vec<String> = diags.iter()
                .map(|d| format!("[{}] {}:{}:{} - {}", d.severity, d.file, d.line, d.character, d.message))
                .collect();
            return Ok(lines.join("\n"));
        }

        // For other commands, get/create a server
        let (lsp_cmd, lsp_args, lang_id) = LspManager::detect_server(file_path);
        let server = self.manager.get_or_create_server(lang_id, lsp_cmd, lsp_args).await?;

        let content = std::fs::read_to_string(file_path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", file_path, e))?;
        server.ensure_document_open(file_path, &content).await?;

        match cmd {
            "symbols" => {
                let symbols = server.document_symbols(file_path).await?;
                Ok(format_symbols_async(&symbols))
            }
            "def" | "definition" => {
                if parts.len() < 4 {
                    return Ok("Usage: lsp def <file> <line> <col>".to_string());
                }
                let line: u32 = parts[2].parse().unwrap_or(0);
                let col: u32 = parts[3].parse().unwrap_or(0);
                let defs = server.goto_definition(file_path, line, col).await?;
                Ok(format_symbols_async(&defs))
            }
            "hover" => {
                if parts.len() < 4 {
                    return Ok("Usage: lsp hover <file> <line> <col>".to_string());
                }
                let line: u32 = parts[2].parse().unwrap_or(0);
                let col: u32 = parts[3].parse().unwrap_or(0);
                match server.hover(file_path, line, col).await? {
                    Some(text) => Ok(text),
                    None => Ok("No hover information found.".to_string()),
                }
            }
            _ => Ok(format!("Unknown lsp command: {}", cmd)),
        }
    }
}

fn format_symbols_async(symbols: &[crate::lsp::Symbol]) -> String {
    if symbols.is_empty() {
        return "No symbols found.".to_string();
    }
    let mut lines = Vec::new();
    for s in symbols {
        let detail = s.detail.as_ref().map(|d| format!(" ({})", d)).unwrap_or_default();
        lines.push(format!("{}:{}:{} | {}{} | {}", s.file, s.line, s.character, s.name, detail, s.kind));
    }
    lines.join("\n")
}
