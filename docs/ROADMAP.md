# OpenShark Master Roadmap — v1.3 to v2.0

> **Goal:** Close every gap against Claude Code, Codex CLI, Cline, and Aider.
> **Approach:** Tier-by-tier implementation. Each feature is isolated, testable, and committed independently.
> **Tech Stack:** Rust 2024, ratatui, crossterm, tokio, serde, toml

---

## TIER 1 — Table Stakes (v1.3)

*High impact, medium effort. These are expected by any serious harness user.*

### 1.1 Auto / YOLO Mode
**Status:** ✅ Shipped in v1.5 | **Effort:** Low | **Source:** Cline, Claude Code

Skip all tool approval prompts. Toggle with `/yolo` or config `yolo_mode = true`.

**Files:**
- Modify: `src/tui/mod.rs` — skip approval popup when `app.yolo_mode`
- Modify: `src/config/mod.rs` — add `yolo_mode: bool` field
- Modify: `src/slash_commands/mod.rs` — add `/yolo` command

**Implementation:**
```rust
// In tool approval handler:
if app.yolo_mode {
    auto_approve = true;
} else {
    show_approval_popup();
}
```

---

### 1.2 Auto-Commit with LLM-Generated Messages
**Status:** ✅ Shipped in v1.5 | **Effort:** Low | **Source:** Aider

After edit tools, generate a commit message via quick LLM call instead of static string.

**Files:**
- Modify: `src/tui/mod.rs` — `auto_commit_changes()` calls LLM for message
- Modify: `src/agent/mod.rs` — add `generate_commit_message(diff: &str) -> String`

**Implementation:**
```rust
async fn generate_commit_message(&self, diff: &str) -> String {
    let prompt = format!("Generate a conventional commit message for this diff:\n{}", diff);
    self.quick_llm_call(prompt).await
}
```

---

### 1.3 Test Runner Auto-Run
**Status:** ✅ Shipped in v1.6 | **Effort:** Medium | **Source:** Aider

After edit tools, auto-run `cargo test` (or configured test command) and report results.

**Files:**
- Modify: `src/tui/mod.rs` — after `auto_commit_changes()`, spawn test runner
- Create: `src/tools/test_runner.rs` — run tests, parse output
- Modify: `src/config/mod.rs` — add `test_command: Option<String>`

**Implementation:**
```rust
async fn run_tests(&self) -> TestResult {
    let cmd = self.config.test_command.as_deref().unwrap_or("cargo test");
    let output = tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await?;
    parse_test_output(output)
}
```

---

### 1.4 Effort Levels / Thinking Budgets
**Status:** ✅ Shipped in v1.4 | **Effort:** Low | **Source:** Claude Code, Aider

`/effort low|medium|high|xhigh` adjusts reasoning depth per request.

**Files:**
- Modify: `src/config/mod.rs` — add `effort_level: String`
- Modify: `src/slash_commands/mod.rs` — add `/effort` command
- Modify: `src/agent/mod.rs` — prefix prompts based on effort level

**Implementation:**
```rust
fn effort_prompt_prefix(level: &str) -> &'static str {
    match level {
        "low" => "Be concise. Minimal explanation.\n",
        "medium" => "Standard detail level.\n",
        "high" => "Thorough analysis with reasoning.\n",
        "xhigh" => "Extremely thorough. Explore edge cases, alternatives, and trade-offs.\n",
        _ => "",
    }
}
```

---

### 1.5 Copy-on-Select
**Status:** ✅ Shipped in v1.4 | **Effort:** Low | **Source:** Claude Code

Auto-copy selected text to clipboard when user releases mouse in TUI.

**Files:**
- Modify: `src/tui/mod.rs` — on mouse release, copy selection to clipboard
- Add dependency: `arboard` crate

**Implementation:**
```rust
// On MouseEvent::Release:
if let Some(selection) = app.current_selection {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(selection)?;
}
```

---

### 1.6 Git Worktree Isolation
**Status:** ✅ Shipped in v1.6 (headless mode) | **Effort:** Medium | **Source:** Claude Code

Background `/headless` sessions use git worktrees so they don't stomp working tree.

**Files:**
- Modify: `src/headless.rs` — create worktree before running, clean up after
- Modify: `src/tools/git.rs` — add `create_worktree(branch: &str) -> PathBuf`

**Implementation:**
```rust
async fn run_in_worktree(task: &str) -> Result<()> {
    let worktree_path = create_worktree(&format!("openshark-{}", uuid::Uuid::new_v4()))?;
    let result = run_task_in_dir(task, &worktree_path).await;
    remove_worktree(&worktree_path)?;
    result
}
```

---

### 1.7 OSC 8 Terminal Hyperlinks
**Status:** ✅ Shipped in v1.4 | **Effort:** Low | **Source:** Codex CLI

Render clickable URLs in terminal output using OSC 8 escape sequences.

**Files:**
- Modify: `src/tui/render.rs` — detect URLs in text, wrap with OSC 8
- Create: `src/utils/osc8.rs` — `osc8_link(url: &str, text: &str) -> String`

**Implementation:**
```rust
pub fn osc8_link(url: &str, text: &str) -> String {
    format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, text)
}
```

---

### 1.8 Context Mode (Auto File Identification)
**Status:** ✅ Shipped in v1.4 | **Effort:** Medium | **Source:** Aider

Agent auto-identifies which files need editing without user specifying.

**Files:**
- Create: `src/agent/context_mode.rs` — analyze request, identify relevant files
- Modify: `src/agent/mod.rs` — if `context_mode` enabled, auto-populate file list
- Modify: `src/config/mod.rs` — add `context_mode: bool`

**Implementation:**
```rust
async fn identify_relevant_files(request: &str, repo_map: &RepoMap) -> Vec<PathBuf> {
    let prompt = format!("Which files in this codebase are relevant to: {}\n\nRepo map:\n{}", request, repo_map);
    let response = llm_call(prompt).await;
    parse_file_list(&response)
}
```

---

## TIER 2 — Power Features (v1.4)

*High impact, high effort. These differentiate OpenShark from basic harnesses.*

### 2.1 Dynamic Workflows / Multi-Agent Orchestration
**Status:** Not started | **Effort:** High | **Source:** Claude Code (`ultracode`)

Spawn 10-100 sub-agents for large tasks, orchestrate results. Think: "Refactor this entire codebase" → spawns agents per module.

**Files:**
- Create: `src/workflows/mod.rs` — workflow engine
- Create: `src/workflows/orchestrator.rs` — spawn/manage sub-agents
- Create: `src/workflows/aggregator.rs` — collect and merge results
- Modify: `src/slash_commands/mod.rs` — add `/workflow` command

**Architecture:**
```rust
struct Workflow {
    tasks: Vec<WorkflowTask>,
    strategy: AggregationStrategy, // Sequential, Parallel, MapReduce
}

struct WorkflowTask {
    agent: AgentConfig,
    prompt: String,
    dependencies: Vec<usize>, // task indices that must complete first
}
```

---

### 2.2 Sandbox v2 — Permission Profiles
**Status:** ✅ Shipped in v1.4 (profiles + sandbox integration) | **Effort:** Medium | **Source:** Codex CLI

Filesystem sandboxing with deny/allow lists. Per-command risk levels.

**Files:**
- Modify: `src/sandbox.rs` — add permission profiles
- Modify: `src/config/mod.rs` — add `sandbox_profile: SandboxProfile`
- Modify: `src/tools/mod.rs` — check permissions before executing

**Implementation:**
```rust
struct SandboxProfile {
    allowed_paths: Vec<PathBuf>,
    denied_paths: Vec<PathBuf>,
    allow_network: bool,
    allow_shell: bool,
    risk_level: RiskLevel, // Low, Medium, High
}
```

---

### 2.3 Guardian Code Review Agent
**Status:** ✅ Shipped in v1.4 (`/review` command) | **Effort:** High | **Source:** Codex CLI

Secondary agent reviews proposed edits before applying. Cache key for speed.

**Files:**
- Create: `src/guardian/mod.rs` — review agent
- Modify: `src/tools/edit.rs` — send diff to guardian before applying
- Modify: `src/tui/mod.rs` — show guardian approval in UI

**Implementation:**
```rust
async fn guardian_review(diff: &str, context: &str) -> ReviewResult {
    let prompt = format!("Review this code change. Approve, request changes, or reject:\n\n{}", diff);
    let review = llm_call_with_cache(prompt, cache_key).await;
    parse_review_result(review)
}
```

---

### 2.4 Voice Mode
**Status:** Not started | **Effort:** Medium | **Source:** Claude Code, Aider

Push-to-talk speech input. Whisper integration.

**Files:**
- Create: `src/voice/mod.rs` — audio capture + Whisper transcription
- Modify: `src/tui/mod.rs` — keybinding for push-to-talk (e.g., hold Space)
- Add dependency: `cpal` (audio), `whisper-rs` (transcription)

**Implementation:**
```rust
// On keydown (hold Space):
start_recording() -> AudioBuffer

// On keyup (release Space):
let transcription = whisper_transcribe(audio_buffer).await;
app.input = transcription;
```

---

### 2.5 Image Paste / Drag-Drop
**Status:** Not started | **Effort:** Medium | **Source:** Claude Code, Aider

Paste screenshots directly into chat. Requires terminal emulator support.

**Files:**
- Create: `src/image_input/mod.rs` — handle paste events
- Modify: `src/tui/mod.rs` — detect image paste, upload to vision model
- Modify: `src/providers/mod.rs` — support vision API

**Implementation:**
```rust
// On paste event:
if is_image_data(&clipboard_content) {
    let image = decode_image(&clipboard_content)?;
    app.add_image_message(image);
    // Send to vision-capable model
}
```

---

### 2.6 Web Scraping (Playwright)
**Status:** Not started | **Effort:** Medium | **Source:** Aider

Ingest web pages as context. Playwright or headless browser integration.

**Files:**
- Create: `src/tools/web_scrape.rs` — fetch and extract page content
- Modify: `src/tools/mod.rs` — register `web_scrape` tool
- Add dependency: `reqwest` + `scraper` (lightweight) or `chromiumoxide` (full browser)

**Implementation:**
```rust
async fn scrape_page(url: &str) -> Result<String> {
    let html = reqwest::get(url).await?.text().await?;
    let text = html_to_markdown(&html)?;
    Ok(text)
}
```

---

### 2.7 Multiple Edit Formats
**Status:** Not started | **Effort:** High | **Source:** Aider

Support diff, udiff, patch, editor-diff formats — not just whole-file rewrite.

**Files:**
- Modify: `src/tools/edit.rs` — support multiple patch formats
- Create: `src/diff/formats.rs` — parse diff/udiff/patch
- Modify: `src/tui/mod.rs` — format selector in UI

**Implementation:**
```rust
enum EditFormat {
    Whole,      // Replace entire file
    Diff,       // Unified diff
    Udiff,      // Context diff
    Patch,      // Git patch
    EditorDiff, // Editor-style hunks
}
```

---

### 2.8 Archive / Unarchive Sessions
**Status:** ✅ Shipped in v1.3 (`/archive`, `/unarchive`) | **Effort:** Low | **Source:** Codex CLI

Save session state to disk, load later. `/archive <name>`, `/unarchive <name>`.

**Files:**
- Create: `src/session/archive.rs` — serialize/deserialize full session state
- Modify: `src/slash_commands/mod.rs` — add `/archive`, `/unarchive`
- Modify: `src/tui/app.rs` — save/load full App state

**Implementation:**
```rust
#[derive(Serialize, Deserialize)]
struct ArchivedSession {
    name: String,
    messages: Vec<Message>,
    config_snapshot: Config,
    created_at: DateTime<Utc>,
}
```

---

### 2.9 Cross-Directory Resume
**Status:** Not started | **Effort:** Medium | **Source:** Claude Code

Resume a session from any directory, not just where it started.

**Files:**
- Modify: `src/session.rs` — store `working_directory` in session metadata
- Modify: `src/tui/mod.rs` — on resume, offer to change directory or stay

**Implementation:**
```rust
// On /resume:
if session.working_directory != current_dir {
    app.add_system_message(format!(
        "This session was started in {}. Current dir is {}. Change directory?",
        session.working_directory, current_dir
    ));
}
```

---

## TIER 3 — Nice-to-Haves (v1.5)

*Medium impact, medium effort. Implement based on user demand.*

### 3.1 Desktop Notifications
**Status:** Not started | **Effort:** Low | **Source:** Claude Code

System notifications when background tasks complete.

**Files:**
- Create: `src/notifications/mod.rs` — cross-platform notifications
- Modify: `src/headless.rs` — notify on completion
- Add dependency: `notify-rust` (Linux), `mac-notification-sys` (macOS)

---

### 3.2 JSON / NDJSON Output Mode
**Status:** ✅ Shipped in v1.6 (headless --json, --ndjson) | **Effort:** Low | **Source:** Claude Code, Cline

`openshark -p "prompt" --json` for scripting/piping.

**Files:**
- Modify: `src/json_output.rs` — ensure full compatibility
- Modify: `src/main.rs` — add `--json` and `--ndjson` CLI flags

---

### 3.3 Team Workflows
**Status:** Not started | **Effort:** Medium | **Source:** Cline

Persistent named agent teams with shared state.

**Files:**
- Create: `src/teams/mod.rs` — team definition and state
- Modify: `src/slash_commands/mod.rs` — add `/team` command

---

### 3.4 Co-Authored-By Attribution
**Status:** Not started | **Effort:** Low | **Source:** Aider

Git commits tagged with `Co-authored-by: OpenShark <openshark@local>`.

**Files:**
- Modify: `src/tui/mod.rs` — append `Co-authored-by` to commit message

---

### 3.5 Watch Mode
**Status:** ✅ Shipped in v1.3 (`/watch` command) | **Effort:** Low | **Source:** Aider

File watcher triggers agent when files change.

**Files:**
- Modify: `src/watch.rs` — integrate with agent loop
- Modify: `src/slash_commands/mod.rs` — add `/watch` command

---

### 3.6 AI Checks for CI/CD
**Status:** Not started | **Effort:** Medium | **Source:** Continue

Markdown-based PR checks. `.openshark/checks/` directory.

**Files:**
- Create: `src/checks/mod.rs` — check runner
- Create: `src/checks/parser.rs` — parse markdown check definitions

---

### 3.7 Config JSON Schema
**Status:** Not started | **Effort:** Low | **Source:** Codex CLI

Validate config against JSON schema.

**Files:**
- Create: `schemas/config.json` — JSON schema for Config struct
- Modify: `src/config/mod.rs` — validate on load

---

### 3.8 PostHog / Analytics
**Status:** Not started | **Effort:** Low | **Source:** Aider

Opt-in usage analytics.

**Files:**
- Create: `src/analytics/mod.rs` — PostHog integration
- Modify: `src/config/mod.rs` — add `analytics_enabled: bool`

---

## TIER 4 — Heavy Lifts (v2.0+)

*Lower impact or massive effort. Only if explicitly requested.*

### 4.1 IDE Extension (VS Code)
**Effort:** Very High | **Source:** Cline, Continue

Full VS Code extension. Separate repo, TypeScript, LSP integration.

### 4.2 OAuth Login Flow
**Effort:** Medium | **Source:** Cline, Claude Code

Web-based auth instead of API keys. Requires backend server.

### 4.3 Native Chat Connectors
**Effort:** Medium | **Source:** Cline, Claude Code

Native Telegram/Slack/Discord bots (vs current gateway approach).

### 4.4 Cron Scheduling
**Effort:** Medium | **Source:** Cline, Claude Code

Built-in recurring tasks. Could leverage Hermes cron.

### 4.5 Hub Daemon / Zen Mode
**Effort:** High | **Source:** Cline

Background task management daemon. `openshark --zen`.

### 4.6 OpenTelemetry
**Effort:** Medium | **Source:** Claude Code

Full metrics, traces, logs pipeline.

### 4.7 Python SDK
**Effort:** High | **Source:** Codex CLI

`pip install openshark` — programmatic API. PyO3 or HTTP API.

### 4.8 Benchmark Mode
**Effort:** Medium | **Source:** Aider

Systematic eval against test suites (HumanEval, SWE-bench).

---

## CHANGELOG TEMPLATE

```markdown
## [1.3.0] — Tier 1 Complete
- feat: Auto/YOLO mode — `/yolo` skips all tool approvals
- feat: Auto-commit with LLM-generated messages
- feat: Test runner auto-run after edits
- feat: Effort levels — `/effort low|medium|high|xhigh`
- feat: Copy-on-select clipboard integration
- feat: Git worktree isolation for background sessions
- feat: OSC 8 terminal hyperlinks
- feat: Context mode — auto-identify files to edit

## [1.4.0] — Tier 2 Complete
- feat: Dynamic workflows — multi-agent orchestration
- feat: Sandbox v2 — permission profiles
- feat: Guardian code review agent
- feat: Voice mode — push-to-talk speech input
- feat: Image paste / drag-drop
- feat: Web scraping with Playwright
- feat: Multiple edit formats (diff, patch, udiff)
- feat: Archive/unarchive sessions
- feat: Cross-directory resume

## [1.5.0] — Tier 3 Complete
- feat: Desktop notifications
- feat: JSON/NDJSON output mode
- feat: Team workflows
- feat: Co-authored-by attribution
- feat: Watch mode
- feat: AI checks for CI/CD
- feat: Config JSON schema validation
- feat: PostHog analytics (opt-in)

## [2.0.0] — Tier 4 Complete
- feat: VS Code extension
- feat: OAuth login flow
- feat: Native chat connectors
- feat: Cron scheduling
- feat: Hub daemon / zen mode
- feat: OpenTelemetry integration
- feat: Python SDK
- feat: Benchmark mode
```

---

## NOTES

- Each feature is independent — can be parallelized across subagents
- TDD where possible — write test first, then implementation
- DRY — reuse existing patterns (e.g., tool approval popup for diff approval)
- YAGNI — don't build plugin UI until plugin system works
- When in doubt, copy Claude Code's UX — it's the gold standard

---

*Last updated: 2026-06-03*
