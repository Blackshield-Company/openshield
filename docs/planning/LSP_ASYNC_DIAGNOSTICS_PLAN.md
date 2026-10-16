# LSP Async + Diagnostics Implementation Plan

## Current Architecture Summary

### LSP Client (`src/lsp/mod.rs`)
- **Fully synchronous, blocking I/O**: Uses `std::process::Child`, `BufReader<ChildStdout>`, and `ChildStdin`
- **Transport**: Raw JSON-RPC over stdio with `Content-Length` framing, using `std::io::Read::read_exact` and `write_all`
- **Thread safety**: `Arc<Mutex<ChildStdin>>` / `Arc<Mutex<BufReader<ChildStdout>>>` — but the mutexes block the thread, not the async runtime
- **Supported LSP methods**: `initialize`, `textDocument/didOpen`, `textDocument/definition`, `textDocument/hover`, `textDocument/documentSymbol`
- **Client lifecycle**: Each tool call spawns a NEW LSP server process (`LspClient::start`), uses it once, then drops it without calling `shutdown()` — the server is leaked
- **Diagnostics struct exists** (`Diagnostic`) but is never populated or used

### Call Sites (all synchronous)
1. **`src/tools/lsp.rs`** (`LspTool::execute`) — Called via `Tool::execute(&self, args: &str) -> Result<String>` (sync trait)
   - Creates `LspClient::start()`, calls `open_document`, then one of: `document_symbols`, `goto_definition`, `hover`
2. **`src/tools/refactor.rs`** (`RefactorTool::execute`) — Same sync trait
   - `extract_function`: `LspClient::start()` → `open_document` → `send_request_sync("textDocument/codeAction")`
   - `rename_symbol`: `LspClient::start()` → `open_document` → `send_request_sync("textDocument/rename")`
   - `inline_variable`: `LspClient::start()` → `open_document` → `send_request_sync("textDocument/codeAction")`

### How Tools Are Invoked
- **Agent loop** (`src/agent/mod.rs:369`): `tool.execute(&step.args)?` — sync call inside an `async fn`, blocks the tokio runtime
- **TUI** (`src/tui/mod.rs:3767`): Uses `AsyncToolExecutor::execute_with_timeout_simple()` which calls `tokio::spawn(async { tool.execute(&args) })` — spawns the sync call on a thread, but the LSP I/O still blocks that thread
- **Headless** (`src/headless.rs:316`): `tool.execute(&suggestion.args)` — sync, blocks
- **CLI subcommands** (`src/main.rs:716, 854, 1327`): Direct sync calls

### Tool Trait (`src/tools/mod.rs`)
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn execute(&self, args: &str) -> Result<String>;  // SYNCHRONOUS
}
```

### Key Problem
Every LSP tool call: spawns a new language server → initializes it → opens one document → makes one request → drops the server (leaked). This is extremely slow (1-5s startup per call) and resource-wasteful.

---

## Files That Need Changes

| File | Change Type |
|------|------------|
| `Cargo.toml` | Add dependencies |
| `src/lsp/mod.rs` | Full rewrite: async transport, persistent connection, notification listener, diagnostics |
| `src/lsp/transport.rs` | NEW: async JSON-RPC transport layer |
| `src/lsp/manager.rs` | NEW: persistent LSP server pool / manager |
| `src/lsp/diagnostics.rs` | NEW: diagnostics collection and reporting |
| `src/tools/lsp.rs` | Rewrite: use async LSP manager, add diagnostics subcommand |
| `src/tools/refactor.rs` | Rewrite: use async LSP manager |
| `src/tools/mod.rs` | Add `AsyncTool` trait, update `find_tool` / registration |
| `src/tools/async.rs` | Update `AsyncToolExecutor` to support `AsyncTool` trait |
| `src/agent/mod.rs` | Update `execute_single_step` to support async tools |
| `src/tui/mod.rs` | Update tool execution paths for async tools |
| `src/headless.rs` | Update tool execution for async tools |
| `src/main.rs` | Update CLI tool invocation paths |
| `src/security/mod.rs` | Update security check for async tool calls |
| `src/linting.rs` | Integrate LSP diagnostics as a linter backend |

---

## New Dependencies

```toml
# Add to Cargo.toml [dependencies]

# LSP protocol types (official Microsoft types)
lsp-types = "0.97"           # Full LSP type definitions

# Async process management
tokio-util = { version = "0.7", features = ["compat"] }  # Async read/write wrappers for child stdio

# Optional: if we want tower-lsp for server-side or testing
# tower-lsp = "0.20"          # Not needed for client-only
```

No `tower-lsp` needed — we are building a **client**, not a server. `lsp-types` gives us proper typed LSP messages instead of raw `serde_json::Value`.

---

## Step-by-Step Implementation Plan

### Step 1: Add Dependencies
**File**: `Cargo.toml`  
**Complexity**: LOW (5 minutes)

Add `lsp-types` and `tokio-util` to `[dependencies]`.

```toml
lsp-types = "0.97"
tokio-util = { version = "0.7", features = ["compat"] }
```

---

### Step 2: Create Async JSON-RPC Transport
**File**: `src/lsp/transport.rs` (NEW)  
**Complexity**: MEDIUM (2-3 hours)

Replace the blocking `BufReader<ChildStdout>` / `ChildStdin` with tokio async I/O.

```rust
// src/lsp/transport.rs
use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use std::sync::Arc;

pub struct AsyncTransport {
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<BufReader<ChildStdout>>>,
}

impl AsyncTransport {
    pub async fn spawn(command: &str, args: &[&str]) -> Result<(Self, Child)> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        Ok((
            Self {
                stdin: Arc::new(Mutex::new(stdin)),
                stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            },
            child,
        ))
    }

    pub async fn send(&self, message: &Value) -> Result<()> { ... }
    pub async fn recv(&self) -> Result<Value> { ... }
}
```

Key design decisions:
- Use `tokio::process::Command` instead of `std::process::Command`
- Use `tokio::sync::Mutex` (not `std::sync::Mutex`) for async-friendly locking
- Parse `Content-Length` header asynchronously with `read_line()`
- Handle both responses and notifications in `recv()`

---

### Step 3: Create LSP Server Manager (Persistent Connections)
**File**: `src/lsp/manager.rs` (NEW)  
**Complexity**: HIGH (4-6 hours)

This is the most impactful change. Instead of spawning a new server per call, maintain a pool.

```rust
// src/lsp/manager.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::lsp::transport::AsyncTransport;

pub struct LspManager {
    servers: Arc<Mutex<HashMap<String, Arc<LspServer>>>>,
    root_path: String,
}

struct LspServer {
    transport: AsyncTransport,
    initialized: bool,
    supports_diagnostics: bool,
    document_versions: HashMap<String, i32>,
}

impl LspManager {
    pub fn new(root_path: &str) -> Self;

    /// Get or create a server for the given language
    pub async fn get_server(&self, language_id: &str) -> Result<Arc<LspServer>>;

    /// Ensure a document is open and up-to-date
    pub async fn ensure_document_open(
        &self,
        server: &LspServer,
        file_path: &str,
        language_id: &str,
    ) -> Result<()>;

    /// Shut down all servers gracefully
    pub async fn shutdown_all(&self) -> Result<()>;

    /// Detect language from file extension
    pub fn detect_language(file_path: &str) -> &'static str;
}

impl LspServer {
    /// Send an LSP request and await the response
    pub async fn request(&self, method: &str, params: Value) -> Result<Value>;

    /// Send an LSP notification (fire-and-forget)
    pub async fn notify(&self, method: &str, params: Value) -> Result<()>;
}
```

The manager:
- Maps language IDs to running server instances
- Reuses connections across multiple tool calls
- Tracks open documents and versions (increments version on changes)
- Calls `shutdown` + `exit` on drop
- Thread-safe via `Arc<Mutex<>>` wrapping the HashMap

---

### Step 4: Add AsyncTool Trait
**File**: `src/tools/mod.rs`  
**Complexity**: MEDIUM (2-3 hours)

Add an async variant of the Tool trait alongside the existing sync one.

```rust
/// Async tool trait for tools that need async I/O (LSP, network, etc.)
#[async_trait::async_trait]
pub trait AsyncTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute_async(&self, args: &str) -> Result<String>;
}

/// Unified enum for tool dispatch
pub enum ToolRef {
    Sync(Arc<dyn Tool>),
    Async(Arc<dyn AsyncTool>),
}

/// Updated find_tool to return ToolRef
pub fn find_tool_ref(name: &str) -> Option<ToolRef>;
```

Update tool registration in `get_tools()` and `get_native_tools()` to include async tools.

---

### Step 5: Rewrite LSP Tool (Async + Diagnostics)
**File**: `src/tools/lsp.rs`  
**Complexity**: MEDIUM (3-4 hours)

Rewrite `LspTool` to implement `AsyncTool` and use the persistent `LspManager`.

```rust
use async_trait::async_trait;
use crate::lsp::manager::LspManager;
use super::AsyncTool;

pub struct LspTool {
    manager: Arc<LspManager>,
}

impl LspTool {
    pub fn new(manager: Arc<LspManager>) -> Self;
}

#[async_trait]
impl AsyncTool for LspTool {
    fn name(&self) -> &str { "lsp" }
    fn description(&self) -> &str { "..." }

    async fn execute_async(&self, args: &str) -> Result<String> {
        // Parse subcommand: symbols, def, hover, diagnostics (NEW)
        // Get server from manager (reuses existing connection)
        // Ensure document is open
        // Execute request
        // Return formatted result
    }
}
```

New subcommand: `diagnostics`
```
lsp diagnostics <file>          - Get current diagnostics for a file
lsp diagnostics /all            - Get diagnostics for all open files
```

---

### Step 6: Rewrite Refactor Tool (Async)
**File**: `src/tools/refactor.rs`  
**Complexity**: MEDIUM (2-3 hours)

Same pattern: implement `AsyncTool`, use `LspManager` for persistent connection.

```rust
#[async_trait]
impl AsyncTool for RefactorTool {
    async fn execute_async(&self, args: &str) -> Result<String> {
        // Same logic but using self.manager.get_server() instead of LspClient::start()
    }
}
```

---

### Step 7: Create Diagnostics Module
**File**: `src/lsp/diagnostics.rs` (NEW)  
**Complexity**: MEDIUM-HIGH (3-4 hours)

Design: Support **both** push and pull diagnostics.

```rust
// src/lsp/diagnostics.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use lsp_types::Diagnostic as LspDiagnostic;

/// Diagnostic event published to subscribers
#[derive(Debug, Clone)]
pub struct DiagnosticEvent {
    pub uri: String,
    pub diagnostics: Vec<crate::lsp::Diagnostic>,
    pub source: String,
}

/// Central diagnostics store
pub struct DiagnosticStore {
    /// Current diagnostics per file URI
    diagnostics: RwLock<HashMap<String, Vec<crate::lsp::Diagnostic>>>,
    /// Subscribers notified on changes
    subscribers: Mutex<Vec<mpsc::UnboundedSender<DiagnosticEvent>>>,
}

impl DiagnosticStore {
    pub fn new() -> Self;

    /// Update diagnostics for a file (from push notification)
    pub async fn update(&self, uri: &str, diagnostics: Vec<crate::lsp::Diagnostic>);

    /// Pull diagnostics for a file (from textDocument/diagnostic request)
    pub async fn pull_diagnostics(
        &self,
        server: &LspServer,
        uri: &str,
    ) -> Result<Vec<crate::lsp::Diagnostic>>;

    /// Get current diagnostics for a file
    pub async fn get(&self, uri: &str) -> Vec<crate::lsp::Diagnostic>;

    /// Get all diagnostics across all files
    pub async fn get_all(&self) -> HashMap<String, Vec<crate::lsp::Diagnostic>>;

    /// Subscribe to diagnostic changes
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<DiagnosticEvent>;
}
```

**Push diagnostics** (preferred for most servers):
- During initialization, the LSP client registers `textDocument/diagnostics` capability or listens for `textDocument/publishDiagnostics` notifications
- A background tokio task reads notifications from the server and updates the `DiagnosticStore`

**Pull diagnostics** (LSP 3.17+):
- Send `textDocument/diagnostic` request to the server
- Requires server to support `diagnosticsProvider` capability
- Fallback if push notifications aren't available

---

### Step 8: Add Notification Listener to Transport
**File**: `src/lsp/transport.rs`, `src/lsp/manager.rs`  
**Complexity**: HIGH (3-4 hours)

The current implementation only reads responses (matching request IDs). For push diagnostics, we need a background listener.

```rust
// In transport.rs, add a notification channel:
pub struct AsyncTransport {
    stdin: Arc<Mutex<ChildStdin>>,
    // Split stdout reading into a dedicated task
    pending_responses: Arc<Mutex<HashMap<i64, tokio::sync::oneshot::Sender<Value>>>>,
    notification_tx: mpsc::UnboundedSender<Value>,  // For notifications like publishDiagnostics
}

impl AsyncTransport {
    /// Start a background reader task
    pub fn start_reader(&self) -> JoinHandle<()> {
        // Reads messages from stdout
        // If message has an `id` field → route to pending_responses
        // If message has a `method` field (notification) → send to notification_tx
    }
}
```

In the manager, the notification listener forwards `textDocument/publishDiagnostics` to the `DiagnosticStore`.

---

### Step 9: Update Agent Loop
**File**: `src/agent/mod.rs`  
**Complexity**: LOW-MEDIUM (1-2 hours)

Update `execute_single_step` to handle async tools:

```rust
async fn execute_single_step(&self, step: &PlanStep, ...) -> Result<String> {
    // ... security checks ...

    // Try async tool first, fall back to sync
    if let Some(ToolRef::Async(tool)) = find_tool_ref(&step.tool_name) {
        tool.execute_async(&step.args).await
    } else if let Some(tool) = find_tool(&step.tool_name) {
        tool.execute(&step.args)
    } else {
        Err(anyhow::anyhow!("Unknown tool: {}", step.tool_name))
    }
}
```

---

### Step 10: Update TUI Tool Execution
**File**: `src/tui/mod.rs`  
**Complexity**: LOW-MEDIUM (1-2 hours)

In `execute_tool_chain` and the auto-execute path (~line 4168), check for async tools:

```rust
// In execute_tool_chain:
if let Some(ToolRef::Async(tool)) = find_tool_ref(tool_name) {
    // Execute with async timeout
    match tokio::time::timeout(
        Duration::from_millis(30000),
        tool.execute_async(args),
    ).await { ... }
} else if let Some(tool) = find_tool(tool_name) {
    // Existing sync path
}
```

---

### Step 11: Update Headless Mode
**File**: `src/headless.rs`  
**Complexity**: LOW (1 hour)

Same pattern as Step 10 — check for async tools before sync tools in the tool execution loop (~line 316).

---

### Step 12: Update AsyncToolExecutor
**File**: `src/tools/async.rs`  
**Complexity**: LOW (1 hour)

Add support for `AsyncTool`:

```rust
pub fn execute_async_tool(&self, tool_name: String, args: String) -> JoinHandle<Result<String>> {
    tokio::spawn(async move {
        if let Some(ToolRef::Async(tool)) = find_tool_ref(&tool_name) {
            tool.execute_async(&args).await
        } else if let Some(tool) = find_tool(&tool_name) {
            tool.execute(&args)
        } else {
            Err(anyhow::anyhow!("Unknown tool: {}", tool_name))
        }
    })
}
```

---

### Step 13: Integrate Diagnostics with Linting
**File**: `src/linting.rs`  
**Complexity**: LOW-MEDIUM (1-2 hours)

Add LSP diagnostics as a linter backend alongside existing clippy/eslint/ruff:

```rust
pub async fn run_linter(path: &str) -> Result<Vec<LintResult>> {
    // Try LSP diagnostics first (faster, in-process)
    if let Ok(diagnostics) = lsp_diagnostics(path).await {
        if !diagnostics.is_empty() {
            return Ok(diagnostics);
        }
    }
    // Fall back to existing linter detection
    let linter = detect_linter(path)...
}

async fn lsp_diagnostics(path: &str) -> Result<Vec<LintResult>> {
    let manager = LspManager::global();
    let store = manager.diagnostics_store();
    let all = store.get_all().await;
    // Convert Diagnostic → LintResult
}
```

---

### Step 14: Wire Up LspManager as Global Singleton
**File**: `src/lsp/mod.rs` (updated)  
**Complexity**: LOW (1 hour)

```rust
// src/lsp/mod.rs
pub mod transport;
pub mod manager;
pub mod diagnostics;

pub use diagnostics::{Diagnostic, DiagnosticStore};
pub use manager::LspManager;
pub use transport::AsyncTransport;

use std::sync::OnceLock;

static LSP_MANAGER: OnceLock<Arc<LspManager>> = OnceLock::new();

pub fn global_lsp_manager() -> Arc<LspManager> {
    LSP_MANAGER.get_or_init(|| {
        Arc::new(LspManager::new("."))
    }).clone()
}
```

---

### Step 15: Update LSP Tool Registration
**File**: `src/tools/mod.rs`  
**Complexity**: LOW (30 minutes)

Change LspTool and RefactorTool from sync `Tool` to `AsyncTool` registration:

```rust
pub fn get_async_tools() -> Vec<Arc<dyn AsyncTool>> {
    let manager = crate::lsp::global_lsp_manager();
    vec![
        Arc::new(lsp::LspTool::new(manager.clone())),
        Arc::new(refactor::RefactorTool::new(manager)),
    ]
}
```

---

## Implementation Order (Recommended)

1. **Step 1**: Add deps to Cargo.toml — unblocks everything
2. **Step 2**: Async transport (`src/lsp/transport.rs`) — foundation
3. **Step 3**: LSP manager (`src/lsp/manager.rs`) — core improvement
4. **Step 4**: AsyncTool trait (`src/tools/mod.rs`) — enables async dispatch
5. **Step 7**: Diagnostic store (`src/lsp/diagnostics.rs`) — data layer
6. **Step 8**: Notification listener — enables push diagnostics
7. **Step 5**: Rewrite LSP tool — uses async manager + diagnostics
8. **Step 6**: Rewrite refactor tool — uses async manager
9. **Step 12**: Update AsyncToolExecutor — unified dispatch
10. **Step 9**: Update agent loop — async tool execution
11. **Step 10**: Update TUI — async in main loop
12. **Step 11**: Update headless — async in headless
13. **Step 14**: Global singleton wiring — lifecycle management
14. **Step 15**: Registration — wire everything together
15. **Step 13**: Linting integration — diagnostics as linter

---

## Complexity Summary

| Step | Description | Complexity | Estimated Time |
|------|-------------|------------|----------------|
| 1 | Add dependencies | LOW | 5 min |
| 2 | Async transport | MEDIUM | 2-3 hr |
| 3 | LSP manager (persistent pool) | HIGH | 4-6 hr |
| 4 | AsyncTool trait | MEDIUM | 2-3 hr |
| 5 | Rewrite LSP tool | MEDIUM | 3-4 hr |
| 6 | Rewrite refactor tool | MEDIUM | 2-3 hr |
| 7 | Diagnostics store | MEDIUM-HIGH | 3-4 hr |
| 8 | Notification listener | HIGH | 3-4 hr |
| 9 | Update agent loop | LOW-MEDIUM | 1-2 hr |
| 10 | Update TUI | LOW-MEDIUM | 1-2 hr |
| 11 | Update headless | LOW | 1 hr |
| 12 | Update AsyncToolExecutor | LOW | 1 hr |
| 13 | Linting integration | LOW-MEDIUM | 1-2 hr |
| 14 | Global singleton wiring | LOW | 1 hr |
| 15 | Tool registration | LOW | 30 min |
| **Total** | | | **~27-38 hr** |

---

## Critical Design Decisions

### 1. AsyncTool vs. Modifying Tool Trait
**Decision**: Add a separate `AsyncTool` trait using `async-trait` crate (already a dependency).
**Rationale**: Modifying `Tool::execute` to be async would require updating ALL 9+ existing tools. A parallel trait allows incremental migration.

### 2. Push vs. Pull Diagnostics
**Decision**: Implement BOTH. Push via notification listener (primary), pull via `textDocument/diagnostic` (fallback).
**Rationale**: Most LSP servers (rust-analyzer, pylsp, gopls) use push diagnostics. LSP 3.17+ adds pull diagnostics. Supporting both maximizes compatibility.

### 3. Persistent vs. Ephemeral Connections
**Decision**: Persistent connections via `LspManager` singleton.
**Rationale**: rust-analyzer takes 2-5s to initialize. Spawning a new server per call (current behavior) makes LSP tools impractically slow for interactive use.

### 4. lsp-types vs. Raw JSON
**Decision**: Use `lsp-types` for typed message construction, keep raw `serde_json::Value` for flexible parsing of responses.
**Rationale**: `lsp-types` provides correct field names and types for requests. Responses can vary between servers, so flexible parsing is still needed.

### 5. Global Singleton vs. Dependency Injection
**Decision**: `OnceLock` global for the `LspManager`, with `Arc<LspManager>` passed to tools.
**Rationale**: The manager needs to outlive individual tool instances and be shared across TUI/headless/agent paths. DI would require threading it through the entire codebase.
