#![allow(dead_code)]

pub mod diagnostics;
pub mod manager;
pub mod transport;

pub use manager::LspManager;
pub use transport::AsyncTransport;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

/// Lightweight LSP client for symbol understanding
pub struct LspClient {
    server: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<BufReader<ChildStdout>>>,
    request_id: Arc<Mutex<i64>>,
    root_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub character: u32,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub message: String,
    pub severity: String,
    pub file: String,
    pub line: u32,
    pub character: u32,
}

impl LspClient {
    pub fn start(command: &str, args: &[&str], root_path: &str) -> Result<Self> {
        let mut server = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to start LSP server: {}", command))?;

        let stdin = server.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("LSP server stdin not available"))?;
        let stdout = server.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("LSP server stdout not available"))?;

        let client = LspClient {
            server,
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            request_id: Arc::new(Mutex::new(0)),
            root_uri: format!("file://{}", std::fs::canonicalize(root_path)?.display()),
        };

        // Send initialize request
        client.initialize()?;

        Ok(client)
    }

    fn next_id(&self) -> i64 {
        let mut id = self.request_id.lock().expect("LSP request_id mutex poisoned");
        *id += 1;
        *id
    }

    pub fn send_request_sync(&self, method: &str, params: Value) -> Result<Value> {
        self.send_request(method, params)
    }

    fn send_request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.send_message(&request)?;
        self.read_response(id)
    }

    fn send_notification(&self, method: &str, params: Value) -> Result<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        self.send_message(&notification)
    }

    fn send_message(&self, message: &Value) -> Result<()> {
        let body = message.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        let mut stdin = self.stdin.lock().expect("LSP stdin mutex poisoned");
        stdin.write_all(header.as_bytes())?;
        stdin.write_all(body.as_bytes())?;
        stdin.flush()?;

        Ok(())
    }

    fn read_response(&self, _expected_id: i64) -> Result<Value> {
        let mut stdout = self.stdout.lock().expect("LSP stdout mutex poisoned");

        let mut content_length: Option<usize> = None;
        let mut header = String::new();
        loop {
            header.clear();
            stdout.read_line(&mut header)?;
            if header == "\r\n" {
                break;
            }
            if let Some(len_str) = header.strip_prefix("Content-Length: ")
                && let Ok(len) = len_str.trim().parse::<usize>()
            {
                content_length = Some(len);
            }
        }

        let len = content_length.context("Missing Content-Length header in LSP response")?;

        let mut _body = String::new();
        {
            let reader = stdout.get_mut();
            let mut buf = vec![0u8; len];
            std::io::Read::read_exact(reader, &mut buf)?;
            _body = String::from_utf8_lossy(&buf).to_string();
        }

        let response: Value = serde_json::from_str(&_body)
            .with_context(|| format!("Failed to parse LSP response: {}", _body))?;

        if let Some(result) = response.get("result") {
            Ok(result.clone())
        } else if let Some(error) = response.get("error") {
            anyhow::bail!("LSP error: {}", error)
        } else {
            Ok(Value::Null)
        }
    }

    fn initialize(&self) -> Result<()> {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": self.root_uri,
            "capabilities": {
                "textDocument": {
                    "hover": { "dynamicRegistration": false },
                    "definition": { "dynamicRegistration": false },
                    "documentSymbol": { "dynamicRegistration": false },
                    "codeAction": { "dynamicRegistration": false }
                }
            }
        });

        self.send_request("initialize", params)?;
        self.send_notification("initialized", json!({}))?;

        Ok(())
    }

    pub fn open_document(&self, file_path: &str, language_id: &str, content: &str) -> Result<()> {
        let uri = format!("file://{}", std::fs::canonicalize(file_path)?.display());

        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": content
                }
            }),
        )
    }

    pub fn goto_definition(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<Symbol>> {
        let uri = format!("file://{}", std::fs::canonicalize(file_path)?.display());

        let result = self.send_request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )?;

        let mut symbols = Vec::new();

        if let Some(arr) = result.as_array() {
            for item in arr {
                if let (Some(uri), Some(range)) =
                    (item.get("uri").and_then(|u| u.as_str()), item.get("range"))
                {
                    let file = uri.strip_prefix("file://").unwrap_or(uri).to_string();
                    let start = range.get("start").unwrap_or(&Value::Null);
                    symbols.push(Symbol {
                        name: String::new(),
                        kind: "definition".to_string(),
                        file,
                        line: start.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
                        character: start.get("character").and_then(|c| c.as_u64()).unwrap_or(0)
                            as u32,
                        detail: None,
                    });
                }
            }
        }

        Ok(symbols)
    }

    pub fn hover(&self, file_path: &str, line: u32, character: u32) -> Result<Option<String>> {
        let uri = format!("file://{}", std::fs::canonicalize(file_path)?.display());

        let result = self.send_request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )?;

        if let Some(contents) = result.get("contents") {
            if let Some(text) = contents.as_str() {
                return Ok(Some(text.to_string()));
            } else if let Some(value) = contents.get("value") {
                return Ok(Some(value.as_str().unwrap_or("").to_string()));
            }
        }

        Ok(None)
    }

    pub fn document_symbols(&self, file_path: &str) -> Result<Vec<Symbol>> {
        let uri = format!("file://{}", std::fs::canonicalize(file_path)?.display());

        let result = self.send_request(
            "textDocument/documentSymbol",
            json!({
                "textDocument": { "uri": uri }
            }),
        )?;

        let mut symbols = Vec::new();

        if let Some(arr) = result.as_array() {
            for item in arr {
                if let (Some(name), Some(kind)) = (
                    item.get("name").and_then(|n| n.as_str()),
                    item.get("kind").and_then(|k| k.as_u64()),
                ) {
                    let location = item.get("location").unwrap_or(&Value::Null);
                    let uri = location.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                    let range = location.get("range").unwrap_or(&Value::Null);
                    let start = range.get("start").unwrap_or(&Value::Null);

                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: symbol_kind_name(kind as u32),
                        file: uri.strip_prefix("file://").unwrap_or(uri).to_string(),
                        line: start.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
                        character: start.get("character").and_then(|c| c.as_u64()).unwrap_or(0)
                            as u32,
                        detail: item
                            .get("detail")
                            .and_then(|d| d.as_str())
                            .map(|s| s.to_string()),
                    });
                }
            }
        }

        Ok(symbols)
    }

    pub fn shutdown(&mut self) -> Result<()> {
        let _ = self.send_request("shutdown", json!({}))?;
        self.send_notification("exit", json!({}))?;
        let _ = self.server.wait();
        Ok(())
    }
}

fn symbol_kind_name(kind: u32) -> String {
    match kind {
        1 => "File",
        2 => "Module",
        3 => "Namespace",
        4 => "Package",
        5 => "Class",
        6 => "Method",
        7 => "Property",
        8 => "Field",
        9 => "Constructor",
        10 => "Enum",
        11 => "Interface",
        12 => "Function",
        13 => "Variable",
        14 => "Constant",
        15 => "String",
        16 => "Number",
        17 => "Boolean",
        18 => "Array",
        19 => "Object",
        20 => "Key",
        21 => "Null",
        22 => "EnumMember",
        23 => "Struct",
        24 => "Event",
        25 => "Operator",
        26 => "TypeParameter",
        _ => "Unknown",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

static LSP_MANAGER: OnceLock<std::sync::Arc<LspManager>> = OnceLock::new();

/// Get the global LSP manager instance (lazily initialized on first access).
pub fn global_lsp_manager() -> std::sync::Arc<LspManager> {
    LSP_MANAGER
        .get_or_init(|| std::sync::Arc::new(LspManager::new(".")))
        .clone()
}
