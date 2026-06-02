# OpenShark: End-All-Be-All Harness — Implementation Roadmap

> **For Hermes:** Use subagent-driven-development skill to implement tasks task-by-task.

**Goal:** Make OpenShark the definitive AI coding harness by closing every gap against competitors (Claude Code, Codex, OpenCode, Hermes, etc.)

**Architecture:** Incremental improvements to existing Rust TUI + backend. Each feature is isolated, testable, and committed independently.

**Tech Stack:** Rust 2024, ratatui, crossterm, tokio, serde, toml

---

## Phase 1: UX Essentials (Week 1)

These are table-stakes features every harness has. Without them, we look amateur.

### Task 1.1: Input History (Up/Down Arrows)

**Objective:** Pressing Up recalls previous user inputs; Down moves forward. Persists across sessions.

**Files:**
- Modify: `src/tui/mod.rs` — key handler for Up/Down
- Modify: `src/tui/app.rs` — add `input_history: Vec<String>` and `history_index: Option<usize>`
- Create: `src/tui/input_history.rs` — history management (load/save/append)

**Step 1: Add history fields to App struct**

```rust
// In src/tui/app.rs, add to App struct:
pub input_history: Vec<String>,
pub history_index: Option<usize>,
pub history_file: PathBuf,
```

**Step 2: Load history on startup**

```rust
// In App::new() or init:
let history_file = config_dir.join("input_history.txt");
let input_history = if history_file.exists() {
    fs::read_to_string(&history_file)
        .unwrap_or_default()
        .lines()
        .map(|s| s.to_string())
        .collect()
} else {
    Vec::new()
};
```

**Step 3: Handle Up/Down in key event handler**

```rust
// In src/tui/mod.rs key handler, replace or add:
KeyCode::Up => {
    if app.input_history.is_empty() { return; }
    let idx = app.history_index.map_or(app.input_history.len() - 1, |i| i.saturating_sub(1));
    app.input = app.input_history[idx].clone();
    app.cursor_position = app.input.len();
    app.history_index = Some(idx);
}
KeyCode::Down => {
    if let Some(idx) = app.history_index {
        if idx + 1 < app.input_history.len() {
            app.input = app.input_history[idx + 1].clone();
            app.cursor_position = app.input.len();
            app.history_index = Some(idx + 1);
        } else {
            app.input.clear();
            app.cursor_position = 0;
            app.history_index = None;
        }
    }
}
```

**Step 4: Save history on submit**

```rust
// When message is submitted:
if !app.input.trim().is_empty() {
    app.input_history.push(app.input.clone());
    // Save to disk
    let _ = fs::write(&app.history_file, app.input_history.join("\n"));
}
```

**Step 5: Commit**

```bash
git add src/tui/app.rs src/tui/mod.rs src/tui/input_history.rs
git commit -m "feat(tui): input history with Up/Down arrows"
```

---

### Task 1.2: Multi-Line Input (Shift+Enter)

**Objective:** Shift+Enter inserts a newline in the input bar instead of submitting.

**Files:**
- Modify: `src/tui/mod.rs` — key handler for Enter with Shift modifier check
- Modify: `src/tui/render.rs` — `draw_input_bar` must render multi-line input

**Step 1: Detect Shift+Enter**

```rust
// In key handler:
KeyCode::Enter => {
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        // Insert newline at cursor
        app.input.insert(app.cursor_position, '\n');
        app.cursor_position += 1;
    } else {
        // Submit message
        submit_message(app);
    }
}
```

**Step 2: Render multi-line input bar**

```rust
// In draw_input_bar, change from single-line to multi-line:
let input_text = if app.input.contains('\n') {
    Text::from(app.input.clone())
} else {
    Text::raw(&app.input)
};
let input_widget = Paragraph::new(input_text)
    .block(input_block)
    .wrap(Wrap { trim: false });
```

**Step 3: Adjust input bar height**

```rust
// In input_bar_height function:
pub fn input_bar_height(input: &str, width: u16) -> u16 {
    let lines = input.matches('\n').count() + 1;
    let wrapped_lines = input.len() / (width as usize).max(1) + 1;
    (lines.max(wrapped_lines) as u16).clamp(3, 10)
}
```

**Step 4: Commit**

```bash
git add src/tui/mod.rs src/tui/render.rs
git commit -m "feat(tui): multi-line input with Shift+Enter"
```

---

### Task 1.3: Undo for File Edits

**Objective:** The `edit` tool saves a backup before applying changes. `/undo` command restores last backup.

**Files:**
- Modify: `src/tools/edit.rs` — save `.openshark_backup` before editing
- Modify: `src/tui/commands.rs` — add `/undo` command handler
- Modify: `src/tui/app.rs` — add `last_backup: Option<(PathBuf, PathBuf)>`

**Step 1: Save backup before edit**

```rust
// In src/tools/edit.rs, before applying patch:
let backup_path = path.with_extension("openshark_backup");
fs::copy(&path, &backup_path)?;
// Store backup info for undo
app.last_backup = Some((path.clone(), backup_path));
```

**Step 2: Add /undo command**

```rust
// In src/tui/commands.rs:
"/undo" => {
    if let Some((original, backup)) = &app.last_backup {
        fs::copy(backup, original)?;
        app.add_system_message("Undo successful".to_string());
    } else {
        app.add_system_message("Nothing to undo".to_string());
    }
}
```

**Step 3: Commit**

```bash
git add src/tools/edit.rs src/tui/commands.rs src/tui/app.rs
git commit -m "feat(tools): undo support for file edits"
```

---

### Task 1.4: Inline Diff Preview

**Objective:** Before applying an edit, show a diff in the chat. User approves or rejects.

**Files:**
- Modify: `src/tools/edit.rs` — generate diff string
- Modify: `src/tui/mod.rs` — show diff in chat, require approval

**Step 1: Generate diff**

```rust
// In src/tools/edit.rs:
use similar::{ChangeTag, TextDiff};

fn generate_diff(original: &str, modified: &str) -> String {
    let diff = TextDiff::from_lines(original, modified);
    let mut output = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        output.push_str(&format!("{}{}", sign, change.value()));
    }
    output
}
```

**Step 2: Show diff and wait for approval**

```rust
// Instead of applying immediately:
let diff = generate_diff(&original_content, &new_content);
app.pending_diff = Some(DiffApproval { path, diff, new_content });
app.mode = AppMode::DiffApproval;
```

**Step 3: Commit**

```bash
git add src/tools/edit.rs src/tui/mod.rs
git commit -m "feat(tools): inline diff preview before edit"
```

---

### Task 1.5: Session Export/Import

**Objective:** `/export <file>` saves current session to JSON. `/import <file>` loads it.

**Files:**
- Create: `src/session/export.rs`
- Modify: `src/tui/commands.rs` — add `/export` and `/import`

**Step 1: Define export format**

```rust
#[derive(Serialize, Deserialize)]
struct SessionExport {
    version: String,
    messages: Vec<Message>,
    metadata: SessionMetadata,
}
```

**Step 2: Implement export/import**

```rust
pub fn export_session(app: &App, path: &Path) -> Result<()> {
    let export = SessionExport {
        version: "1.0".to_string(),
        messages: app.messages.clone(),
        metadata: app.session_metadata.clone(),
    };
    let json = serde_json::to_string_pretty(&export)?;
    fs::write(path, json)?;
    Ok(())
}
```

**Step 3: Commit**

```bash
git add src/session/export.rs src/tui/commands.rs
git commit -m "feat(session): export and import sessions"
```

---

## Phase 2: Power User Features (Week 2)

### Task 2.1: Token Counter Display

**Objective:** Show real-time token count in input bar (using tiktoken-rs or approximate).

**Files:**
- Modify: `src/tui/render.rs` — `draw_input_bar` shows token count
- Add: `src/utils/tokens.rs` — token estimation

---

### Task 2.2: Cost Tracker

**Objective:** Track running cost per session based on model pricing. Show in sidebar.

**Files:**
- Modify: `src/router/mod.rs` — add pricing table per model
- Modify: `src/tui/app.rs` — accumulate cost
- Modify: `src/tui/render.rs` — show cost in sidebar

---

### Task 2.3: Search in Chat History

**Objective:** `/search <query>` finds messages in current session. Highlight matches.

**Files:**
- Modify: `src/tui/commands.rs` — `/search` command
- Modify: `src/tui/render.rs` — highlight search matches

---

### Task 2.4: Request Replay

**Objective:** `/replay` resends the last user message to a different model for comparison.

**Files:**
- Modify: `src/tui/commands.rs` — `/replay [model]`
- Modify: `src/tui/app.rs` — store last user message

---

### Task 2.5: Custom Keybindings

**Objective:** Keybindings configurable in `~/.config/openshark/keybindings.toml`.

**Files:**
- Create: `src/config/keybindings.rs`
- Modify: `src/tui/mod.rs` — read keybindings from config instead of hardcoded

---

## Phase 3: Safety & Sandbox (Week 3)

### Task 3.1: Git Commit Integration

**Objective:** `/commit "message"` stages all changes and commits. `/pr` creates a PR.

**Files:**
- Create: `src/tools/git_commit.rs`
- Modify: `src/tui/commands.rs`

---

### Task 3.2: Docker Sandbox for Terminal

**Objective:** Optional Docker container for `terminal` tool execution. Configurable per-command risk level.

**Files:**
- Create: `src/security/docker.rs`
- Modify: `src/tools/terminal.rs` — run in container if enabled

---

### Task 3.3: Health Check Dashboard

**Objective:** Sidebar shows provider status (green/yellow/red) with latency.

**Files:**
- Modify: `src/router/mod.rs` — health check polling
- Modify: `src/tui/render.rs` — status indicators in sidebar

---

## Phase 4: Extensibility (Week 4)

### Task 4.1: Plugin/WASM System

**Objective:** Load `.wasm` plugins from `~/.config/openshark/plugins/`. Plugins register new tools.

**Files:**
- Create: `src/plugins/mod.rs`
- Create: `src/plugins/wasm.rs`
- Modify: `src/tools/mod.rs` — dynamic tool registration

---

### Task 4.2: Benchmark Harness

**Objective:** `/benchmark <suite>` runs evals (HumanEval, SWE-bench, custom). Reports scores per model.

**Files:**
- Create: `src/benchmark/mod.rs`
- Create: `src/benchmark/humaneval.rs`
- Modify: `src/tui/commands.rs`

---

### Task 4.3: A/B Prompt Testing

**Objective:** Evolution engine automatically A/B tests prompt variations and picks winners.

**Files:**
- Modify: `src/evolution/mod.rs` — add A/B test framework
- Modify: `src/tui/app.rs` — track prompt variants

---

## Acceptance Criteria

- [ ] All Phase 1 tasks complete and committed
- [ ] Each feature has basic manual test verification
- [ ] No regressions in existing functionality (354 tests pass)
- [ ] Documentation updated in README.md
- [ ] CHANGELOG.md updated

## Notes

- Each task is independent — can be parallelized across subagents
- TDD where possible — write test first, then implementation
- DRY — reuse existing patterns (e.g., tool approval popup for diff approval)
- YAGNI — don't build plugin UI until plugin system works
