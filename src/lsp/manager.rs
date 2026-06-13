//! LSP server manager — maintains a pool of persistent LSP server connections
//! keyed by language ID, avoiding per-call server spawns.

use super::Symbol;
use super::diagnostics::{DiagnosticStore, parse_diagnostics_notification};
use crate::lsp::AsyncTransport;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// LspManager
// ---------------------------------------------------------------------------

/// Manages a pool of persistent LSP server connections.
///
/// Each language gets at most one running server. Servers are spawned lazily
/// on first use and kept alive for the lifetime of the manager (or until
/// explicitly shut down).
pub struct LspManager {
    servers: Mutex<HashMap<String, Arc<LspServer>>>,
    root_path: String,
    diagnostics: Arc<DiagnosticStore>,
}

impl LspManager {
    /// Create a new manager rooted at `root_path`.
    pub fn new(root_path: &str) -> Self {
        Self {
            servers: Mutex::new(HashMap::new()),
            root_path: root_path.to_string(),
            diagnostics: Arc::new(DiagnosticStore::new()),
        }
    }

    /// Return the language ID for a file based on its extension.
    pub fn detect_language(file_path: &str) -> &'static str {
        if file_path.ends_with(".rs") {
            "rust"
        } else if file_path.ends_with(".py") {
            "python"
        } else if file_path.ends_with(".js") || file_path.ends_with(".ts") {
            "typescript"
        } else if file_path.ends_with(".go") {
            "go"
        } else if file_path.ends_with(".c")
            || file_path.ends_with(".cpp")
            || file_path.ends_with(".h")
        {
            "cpp"
        } else {
            "rust" // default
        }
    }

    /// Detect the LSP server command, arguments, and language ID for a file.
    pub fn detect_server(file_path: &str) -> (&'static str, &'static [&'static str], &'static str) {
        if file_path.ends_with(".rs") {
            ("rust-analyzer", &[], "rust")
        } else if file_path.ends_with(".py") {
            ("pylsp", &[], "python")
        } else if file_path.ends_with(".js") || file_path.ends_with(".ts") {
            ("typescript-language-server", &["--stdio"], "typescript")
        } else if file_path.ends_with(".go") {
            ("gopls", &[], "go")
        } else if file_path.ends_with(".c")
            || file_path.ends_with(".cpp")
            || file_path.ends_with(".h")
        {
            ("clangd", &[], "cpp")
        } else {
            ("rust-analyzer", &[], "rust") // default
        }
    }

    /// Get an existing server for `language_id` or spawn + initialize a new one.
    pub async fn get_or_create_server(
        &self,
        language_id: &str,
        lsp_cmd: &str,
        lsp_args: &[&str],
    ) -> Result<Arc<LspServer>> {
        // Fast path: already have a server for this language.
        {
            let servers = self.servers.lock().await;
            if let Some(server) = servers.get(language_id) {
                return Ok(Arc::clone(server));
            }
        }

        // Slow path: spawn and initialize.
        let server = LspServer::spawn(
            language_id,
            lsp_cmd,
            lsp_args,
            &self.root_path,
            Arc::clone(&self.diagnostics),
        )
        .await?;

        let server = Arc::new(server);

        {
            let mut servers = self.servers.lock().await;
            // Another task may have raced us — prefer the existing one.
            if let Some(existing) = servers.get(language_id) {
                return Ok(Arc::clone(existing));
            }
            servers.insert(language_id.to_string(), Arc::clone(&server));
        }

        Ok(server)
    }

    /// Get a handle to the shared diagnostic store.
    pub fn diagnostics_store(&self) -> Arc<DiagnosticStore> {
        Arc::clone(&self.diagnostics)
    }

    /// Gracefully shut down every managed server.
    pub async fn shutdown_all(&self) -> Result<()> {
        let mut servers = self.servers.lock().await;
        for (lang, server) in servers.drain() {
            debug!("Shutting down LSP server for language: {lang}");
            if let Err(e) = server.shutdown().await {
                warn!("Error shutting down LSP server for {lang}: {e:#}");
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LspServer
// ---------------------------------------------------------------------------

/// A single persistent LSP server connection.
pub(crate) struct LspServer {
    transport: AsyncTransport,
    child: Mutex<Option<tokio::process::Child>>,
    language_id: String,
    initialized: bool,
    open_documents: Mutex<HashSet<String>>,
    document_versions: Mutex<HashMap<String, i32>>,
}

impl LspServer {
    /// Spawn the server process, initialize it, and start the background
    /// diagnostics-forwarding task.
    async fn spawn(
        language_id: &str,
        lsp_cmd: &str,
        lsp_args: &[&str],
        root_path: &str,
        diagnostics: Arc<DiagnosticStore>,
    ) -> Result<Self> {
        let (transport, child) = AsyncTransport::spawn(lsp_cmd, lsp_args).await?;

        let mut server = Self {
            transport,
            child: Mutex::new(Some(child)),
            language_id: language_id.to_string(),
            initialized: false,
            open_documents: Mutex::new(HashSet::new()),
            document_versions: Mutex::new(HashMap::new()),
        };

        // Initialize the LSP session.
        server.initialize(root_path).await?;
        server.initialized = true;

        // Start the background task that forwards publishDiagnostics
        // notifications to the DiagnosticStore.
        let mut notification_rx = server.transport.notifications();
        tokio::spawn(async move {
            loop {
                match notification_rx.recv().await {
                    Ok(notification) => {
                        let method = notification
                            .get("method")
                            .and_then(|m| m.as_str())
                            .unwrap_or("");

                        if method == "textDocument/publishDiagnostics"
                            && let Some(params) = notification.get("params")
                            && let Some((uri, diags)) = parse_diagnostics_notification(params)
                        {
                            diagnostics.update(&uri, diags).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Diagnostics listener lagged, skipped {n} notifications");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        debug!("Diagnostics notification channel closed, exiting listener");
                        break;
                    }
                }
            }
        });

        Ok(server)
    }

    /// Ensure a document is open in the LSP server.
    ///
    /// If the document is not yet open, sends `textDocument/didOpen`.
    /// If it is already open, sends `textDocument/didChange` with an
    /// incremented version number.
    pub async fn ensure_document_open(&self, file_path: &str, content: &str) -> Result<()> {
        let uri = file_path_to_uri(file_path)?;

        let mut open_docs = self.open_documents.lock().await;
        if open_docs.contains(file_path) {
            // Already open — send didChange with incremented version.
            drop(open_docs); // release lock before async call

            let version = {
                let mut versions = self.document_versions.lock().await;
                let v = versions.entry(file_path.to_string()).or_insert(1);
                *v += 1;
                *v
            };

            self.transport
                .send_notification(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "version": version,
                        },
                        "contentChanges": [
                            { "text": content }
                        ]
                    }),
                )
                .await?;
        } else {
            // Not yet open — send didOpen.
            let version = {
                let mut versions = self.document_versions.lock().await;
                versions.insert(file_path.to_string(), 1);
                1
            };

            self.transport
                .send_notification(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": self.language_id,
                            "version": version,
                            "text": content,
                        }
                    }),
                )
                .await?;

            open_docs.insert(file_path.to_string());
        }

        Ok(())
    }

    /// Send a JSON-RPC request and return the response.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.transport.send_request(method, params).await
    }

    /// Send a JSON-RPC notification (fire-and-forget).
    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.transport.send_notification(method, params).await
    }

    /// Go to definition at the given position.
    pub async fn goto_definition(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<Symbol>> {
        let uri = file_path_to_uri(file_path)?;

        let result = self
            .transport
            .send_request(
                "textDocument/definition",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                }),
            )
            .await?;

        let mut symbols = Vec::new();

        // The LSP spec allows several response shapes:
        //   1. array of Location
        //   2. a single Location (object with uri + range)
        //   3. array of LocationLink
        //   4. null
        if let Some(arr) = result.as_array() {
            for item in arr {
                if let Some(sym) = parse_location_or_link(item) {
                    symbols.push(sym);
                }
            }
        } else if result.is_object() && result.get("uri").is_some() {
            // Single Location
            if let Some(sym) = parse_location_or_link(&result) {
                symbols.push(sym);
            }
        }

        Ok(symbols)
    }

    /// Hover at the given position.
    pub async fn hover(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<String>> {
        let uri = file_path_to_uri(file_path)?;

        let result = self
            .transport
            .send_request(
                "textDocument/hover",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                }),
            )
            .await?;

        if let Some(contents) = result.get("contents") {
            if let Some(text) = contents.as_str() {
                return Ok(Some(text.to_string()));
            } else if let Some(value) = contents.get("value") {
                return Ok(Some(value.as_str().unwrap_or("").to_string()));
            }
            // MarkedString[] — concatenate string entries.
            if let Some(arr) = contents.as_array() {
                let mut parts = Vec::new();
                for item in arr {
                    if let Some(s) = item.as_str() {
                        parts.push(s.to_string());
                    } else if let Some(v) = item.get("value").and_then(|v| v.as_str()) {
                        parts.push(v.to_string());
                    }
                }
                if !parts.is_empty() {
                    return Ok(Some(parts.join("\n\n")));
                }
            }
        }

        Ok(None)
    }

    /// Get document symbols for a file.
    pub async fn document_symbols(&self, file_path: &str) -> Result<Vec<Symbol>> {
        let uri = file_path_to_uri(file_path)?;

        let result = self
            .transport
            .send_request(
                "textDocument/documentSymbol",
                json!({
                    "textDocument": { "uri": uri }
                }),
            )
            .await?;

        let mut symbols = Vec::new();

        if let Some(arr) = result.as_array() {
            for item in arr {
                if let (Some(name), Some(kind)) = (
                    item.get("name").and_then(|n| n.as_str()),
                    item.get("kind").and_then(|k| k.as_u64()),
                ) {
                    // DocumentSymbol has range directly; DocumentSymbol
                    // with a "location" field is the SymbolInformation variant.
                    let location = item.get("location").unwrap_or(&Value::Null);
                    let range = if location.is_null() {
                        item.get("range").unwrap_or(&Value::Null)
                    } else {
                        location.get("range").unwrap_or(&Value::Null)
                    };
                    let start = range.get("start").unwrap_or(&Value::Null);

                    let file_uri = location.get("uri").and_then(|u| u.as_str()).unwrap_or(&uri);

                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: symbol_kind_name(kind as u32),
                        file: file_uri
                            .strip_prefix("file://")
                            .unwrap_or(file_uri)
                            .to_string(),
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

    /// Send `initialize` + `initialized` to the server.
    async fn initialize(&self, root_path: &str) -> Result<()> {
        let root_uri = format!(
            "file://{}",
            std::fs::canonicalize(root_path)
                .unwrap_or_else(|_| root_path.into())
                .display()
        );

        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "hover": { "dynamicRegistration": false },
                    "definition": { "dynamicRegistration": false },
                    "documentSymbol": { "dynamicRegistration": false },
                    "codeAction": { "dynamicRegistration": false },
                    "publishDiagnostics": { "relatedInformation": true }
                }
            }
        });

        self.transport
            .send_request("initialize", params)
            .await
            .context("LSP initialize request failed")?;

        self.transport
            .send_notification("initialized", json!({}))
            .await
            .context("LSP initialized notification failed")?;

        Ok(())
    }

    /// Gracefully shut down the server: send `shutdown` then `exit`.
    pub async fn shutdown(&self) -> Result<()> {
        if let Err(e) = self.transport.send_request("shutdown", json!({})).await {
            warn!("LSP shutdown request failed: {e:#}");
        }
        if let Err(e) = self.transport.send_notification("exit", json!({})).await {
            warn!("LSP exit notification failed: {e:#}");
        }

        // Kill the child process if it is still running.
        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            let _ = child.kill().await;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a filesystem path to a `file://` URI, canonicalizing if possible.
fn file_path_to_uri(path: &str) -> Result<String> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    Ok(format!("file://{}", canonical.display()))
}

/// Parse a Location or LocationLink JSON object into a Symbol.
fn parse_location_or_link(item: &Value) -> Option<Symbol> {
    // LocationLink uses "targetUri" / "targetRange".
    // Location uses "uri" / "range".
    let (uri, range) = if let Some(target_uri) = item.get("targetUri").and_then(|u| u.as_str()) {
        let range = item.get("targetRange").unwrap_or(&Value::Null);
        (target_uri, range)
    } else {
        let uri = item.get("uri").and_then(|u| u.as_str())?;
        let range = item.get("range").unwrap_or(&Value::Null);
        (uri, range)
    };

    let start = range.get("start").unwrap_or(&Value::Null);

    Some(Symbol {
        name: String::new(),
        kind: "definition".to_string(),
        file: uri.strip_prefix("file://").unwrap_or(uri).to_string(),
        line: start.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
        character: start.get("character").and_then(|c| c.as_u64()).unwrap_or(0) as u32,
        detail: None,
    })
}

/// Map an LSP SymbolKind numeric code to a human-readable name.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(LspManager::detect_language("foo.rs"), "rust");
        assert_eq!(LspManager::detect_language("bar.py"), "python");
        assert_eq!(LspManager::detect_language("baz.js"), "typescript");
        assert_eq!(LspManager::detect_language("baz.ts"), "typescript");
        assert_eq!(LspManager::detect_language("main.go"), "go");
        assert_eq!(LspManager::detect_language("foo.c"), "cpp");
        assert_eq!(LspManager::detect_language("foo.cpp"), "cpp");
        assert_eq!(LspManager::detect_language("foo.h"), "cpp");
        assert_eq!(LspManager::detect_language("unknown.txt"), "rust"); // default
    }

    #[test]
    fn test_detect_server() {
        let (cmd, args, lang) = LspManager::detect_server("test.rs");
        assert_eq!(cmd, "rust-analyzer");
        assert_eq!(lang, "rust");
        assert!(args.is_empty());

        let (cmd, _args, lang) = LspManager::detect_server("test.py");
        assert_eq!(cmd, "pylsp");
        assert_eq!(lang, "python");

        let (cmd, args, lang) = LspManager::detect_server("app.ts");
        assert_eq!(cmd, "typescript-language-server");
        assert_eq!(args, &["--stdio"]);
        assert_eq!(lang, "typescript");

        let (cmd, _args, lang) = LspManager::detect_server("main.go");
        assert_eq!(cmd, "gopls");
        assert_eq!(lang, "go");

        let (cmd, _args, lang) = LspManager::detect_server("foo.c");
        assert_eq!(cmd, "clangd");
        assert_eq!(lang, "cpp");
    }

    #[test]
    fn test_symbol_kind_name() {
        assert_eq!(symbol_kind_name(5), "Class");
        assert_eq!(symbol_kind_name(6), "Method");
        assert_eq!(symbol_kind_name(12), "Function");
        assert_eq!(symbol_kind_name(13), "Variable");
        assert_eq!(symbol_kind_name(23), "Struct");
        assert_eq!(symbol_kind_name(99), "Unknown");
    }

    #[test]
    fn test_parse_location_or_link_location() {
        let val = serde_json::json!({
            "uri": "file:///tmp/test.rs",
            "range": {
                "start": { "line": 10, "character": 5 },
                "end": { "line": 10, "character": 10 }
            }
        });
        let sym = parse_location_or_link(&val).unwrap();
        assert_eq!(sym.file, "/tmp/test.rs");
        assert_eq!(sym.line, 10);
        assert_eq!(sym.character, 5);
        assert_eq!(sym.kind, "definition");
    }

    #[test]
    fn test_parse_location_or_link_location_link() {
        let val = serde_json::json!({
            "targetUri": "file:///tmp/test.rs",
            "targetRange": {
                "start": { "line": 20, "character": 3 },
                "end": { "line": 20, "character": 8 }
            }
        });
        let sym = parse_location_or_link(&val).unwrap();
        assert_eq!(sym.file, "/tmp/test.rs");
        assert_eq!(sym.line, 20);
        assert_eq!(sym.character, 3);
    }

    #[tokio::test]
    async fn test_manager_new() {
        let manager = LspManager::new("/tmp");
        assert!(
            manager
                .diagnostics_store()
                .get("/tmp/nonexistent.rs")
                .await
                .is_empty()
        );
    }
}
