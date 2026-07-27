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
    /// Last bytes of the server's stderr — included in EOF errors so a
    /// server that dies on startup (missing rustup component, bad flags)
    /// tells us WHY instead of just closing the pipe.
    stderr_buf: Arc<Mutex<String>>,
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

/// True if `command` resolves to an executable file on PATH.
pub fn command_on_path(command: &str) -> bool {
    if command.contains('/') {
        return std::path::Path::new(command).is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(command).is_file())
        })
        .unwrap_or(false)
}

/// Install hint for a known LSP server binary (Arch/CachyOS-flavored).
pub fn lsp_server_install_hint(command: &str) -> Option<&'static str> {
    match command {
        "pylsp" => Some("sudo pacman -S python-lsp-server  (or: pipx install python-lsp-server)"),
        "typescript-language-server" => {
            Some("npm install -g typescript-language-server typescript")
        }
        "gopls" => Some("go install golang.org/x/tools/gopls@latest"),
        "rust-analyzer" => Some("rustup component add rust-analyzer"),
        "clangd" => Some("sudo pacman -S clang"),
        _ => None,
    }
}

/// Fail fast with an actionable message when the server binary is missing,
/// instead of the opaque "Failed to start LSP server: <cmd>" spawn error.
pub fn ensure_lsp_server(command: &str) -> Result<()> {
    if command_on_path(command) {
        return Ok(());
    }
    let hint = lsp_server_install_hint(command)
        .map(|h| format!(" Install: {h}"))
        .unwrap_or_default();
    Err(anyhow::anyhow!(
        "LSP server '{command}' is not installed (not found in PATH).{hint}"
    ))
}

impl LspClient {
    pub fn start(command: &str, args: &[&str], root_path: &str) -> Result<Self> {
        ensure_lsp_server(command)?;
        let mut server = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to start LSP server: {}", command))?;

        let stdin = server.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("LSP server stdin not available"))?;
        let stdout = server.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("LSP server stdout not available"))?;

        // Drain stderr into a small ring buffer so EOF errors can quote it.
        let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        if let Some(mut stderr) = server.stderr.take() {
            let buf = Arc::clone(&stderr_buf);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 1024];
                loop {
                    match std::io::Read::read(&mut stderr, &mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut b) = buf.lock() {
                                b.push_str(&String::from_utf8_lossy(&chunk[..n]));
                                // Cap at 4KB — keep the tail, that's where errors live
                                if b.len() > 4096 {
                                    let keep = b.len() - 4096;
                                    b.drain(..keep);
                                }
                            }
                        }
                    }
                }
            });
        }

        let client = LspClient {
            server,
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            request_id: Arc::new(Mutex::new(0)),
            root_uri: format!("file://{}", std::fs::canonicalize(root_path)?.display()),
            stderr_buf,
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

    /// Read exactly one Content-Length framed message from the server.
    /// MUST read the body through the BufReader (`stdout`) — never via
    /// `get_mut()` on the underlying fd. BufReader swallows the whole pipe
    /// chunk (headers + body) into its internal buffer; bypassing it blocks
    /// forever waiting for data that is already sitting in the buffer.
    fn read_message(&self, stdout: &mut BufReader<ChildStdout>) -> Result<Value> {
        let mut content_length: Option<usize> = None;
        let mut header = String::new();
        loop {
            header.clear();
            let n = stdout.read_line(&mut header)?;
            if n == 0 {
                let stderr_tail = self
                    .stderr_buf
                    .lock()
                    .map(|b| b.trim().to_string())
                    .unwrap_or_default();
                if stderr_tail.is_empty() {
                    anyhow::bail!(
                        "LSP server closed stdout (EOF) while reading headers — the server exited. Verify the server binary runs standalone."
                    );
                }
                anyhow::bail!(
                    "LSP server closed stdout (EOF) while reading headers — the server exited. Server stderr: {}",
                    stderr_tail
                );
            }
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

        let mut buf = vec![0u8; len];
        std::io::Read::read_exact(stdout, &mut buf)?;
        let body = String::from_utf8_lossy(&buf).to_string();

        serde_json::from_str(&body)
            .with_context(|| format!("Failed to parse LSP response: {}", body))
    }

    fn read_response(&self, expected_id: i64) -> Result<Value> {
        let mut stdout = self.stdout.lock().expect("LSP stdout mutex poisoned");

        // Skip interleaved messages (publishDiagnostics notifications, log
        // messages, server→client requests) until OUR response arrives.
        // pylsp routinely sends publishDiagnostics before the actual result.
        loop {
            let message = self.read_message(&mut stdout)?;
            let id = message.get("id").and_then(|v| v.as_i64());
            if id != Some(expected_id) {
                continue;
            }
            if let Some(result) = message.get("result") {
                return Ok(result.clone());
            }
            if let Some(error) = message.get("error") {
                anyhow::bail!("LSP error: {}", error);
            }
            return Ok(Value::Null);
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

#[cfg(test)]
mod lsp_server_preflight_tests {
    use super::*;

    #[test]
    fn command_on_path_finds_sh() {
        assert!(command_on_path("sh"));
    }

    #[test]
    fn command_on_path_rejects_missing_binary() {
        assert!(!command_on_path("definitely-not-a-real-lsp-server-xyz"));
    }

    #[test]
    fn ensure_lsp_server_error_names_binary_and_hint() {
        let err = ensure_lsp_server("pylsp-nonexistent-variant").unwrap_err();
        assert!(err.to_string().contains("not installed"));
        // Unknown binary → no hint, but still a clear message
        assert!(!err.to_string().contains("Install:"));
    }

    #[test]
    fn install_hints_cover_every_detected_server() {
        // Every server named in the detect_server tables must have a hint —
        // otherwise the model gets "not installed" with no way forward.
        for cmd in [
            "rust-analyzer",
            "pylsp",
            "typescript-language-server",
            "gopls",
            "clangd",
        ] {
            assert!(
                lsp_server_install_hint(cmd).is_some(),
                "missing install hint for {cmd}"
            );
        }
    }
}

#[cfg(test)]
mod lsp_live_tests {
    //! Live protocol tests against a real pylsp process.
    //! Run: cargo test --bin openshark lsp_live -- --include-ignored --nocapture
    use super::*;
    use crate::lsp::LspManager;

    const DEMO: &str = "demo.py";

    fn demo_content() -> String {
        "def greet(name: str) -> str:\n    \"\"\"Return a retro greeting.\"\"\"\n    return f\"Stay retro, {name}\"\n\n\nmessage = greet(\"openshark\")\nprint(message)\n"
            .to_string()
    }

    /// Self-contained scratch dir + python file (tests must not depend on
    /// anything outside the repo/target tmp dirs).
    fn setup_python_file() -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join("lsp_check_py");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(DEMO);
        std::fs::write(&file, demo_content()).unwrap();
        (dir, file.to_string_lossy().to_string())
    }

    #[test]
    #[ignore = "requires pylsp installed"]
    fn sync_client_hover_python() {
        let (dir, file) = setup_python_file();
        let client = LspClient::start("pylsp", &[], dir.to_str().unwrap()).unwrap();
        client
            .open_document(&file, "python", &demo_content())
            .unwrap();
        let hover = client.hover(&file, 5, 12).unwrap();
        println!("sync hover result: {hover:?}");
        // Regression: sync read_response used to return the FIRST message —
        // often an interleaved publishDiagnostics notification — and lose the
        // real hover result.
        assert!(
            hover.is_some(),
            "sync client lost the hover result to an interleaved notification"
        );
    }

    #[tokio::test]
    #[ignore = "requires pylsp installed"]
    async fn async_manager_hover_python() {
        let (dir, file) = setup_python_file();
        let manager = LspManager::new(dir.to_str().unwrap());
        let server = manager
            .get_or_create_server("python", "pylsp", &[])
            .await
            .unwrap();
        server
            .ensure_document_open(&file, &demo_content())
            .await
            .unwrap();
        let hover = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            server.hover(&file, 5, 12),
        )
        .await
        .expect("async hover wedged — 30s timeout")
        .unwrap();
        println!("async hover result: {hover:?}");
        assert!(hover.is_some(), "async manager returned no hover");
        manager.shutdown_all().await.unwrap();
    }

    #[test]
    #[ignore = "requires rust-analyzer component (rustup component add rust-analyzer)"]
    fn sync_client_hover_rust() {
        // rust-analyzer is a rustup PROXY: the binary exists on PATH even when
        // the component is missing, in which case it prints "Unknown binary
        // 'rust-analyzer' in official toolchain" to stderr and exits → EOF.
        // The EOF error must quote that stderr, not just say "closed stdout".
        //
        // NOTE: rust-analyzer gives full semantics only inside a cargo
        // project — a bare .rs file ("detached file") gets no hover. The
        // scratch project below mirrors real usage.
        let dir = std::env::temp_dir().join("lsp_check_rs");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"lsp_check_rs\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let file = dir.join("src/main.rs");
        std::fs::write(
            &file,
            "fn main() {\n    let msg = \"openshark protocol test\";\n    println!(\"{}\", msg);\n}\n",
        )
        .unwrap();
        let file_str = file.to_string_lossy().to_string();

        let client = LspClient::start("rust-analyzer", &[], dir.to_str().unwrap()).unwrap();
        let content = std::fs::read_to_string(&file).unwrap();
        client.open_document(&file_str, "rust", &content).unwrap();
        // rust-analyzer needs time to load the project model; poll the hover.
        // -32801 (ContentModified) is transient during startup — r-a reloads
        // the file from disk and bumps its internal version, making our
        // didOpen version momentarily stale.
        let mut hover = None;
        for _ in 0..15 {
            match client.hover(&file_str, 2, 20) {
                Ok(Some(h)) => {
                    hover = Some(h);
                    break;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_secs(2)),
                Err(e) if e.to_string().contains("-32801") => {
                    std::thread::sleep(std::time::Duration::from_secs(2))
                }
                Err(e) => panic!("hover errored (EOF/crash?): {e:#}"),
            }
        }
        println!("rust hover result: {hover:?}");
        assert!(hover.is_some(), "rust-analyzer returned no hover");
    }
}
