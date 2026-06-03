# AI Coding Harness Research — Feature Matrix

Research conducted: 2026-06-02 | Updated: 2026-06-03
Harnesses analyzed: Claude Code, Codex CLI, Cline, Aider, Continue, OpenShark (self)

---

## LEGEND

- [x] = Implemented in OpenShark v1.2+
- [ ] = Not yet implemented
- ~[ ]~ = Partially implemented / MVP version exists

---

## 1. CLAUDE CODE (Anthropic) — v2.1.161

### Core Features
- **Agentic coding** with natural language commands
- [x] **Plan/Act mode toggle** — `/plan` explores, `/act` executes
- [x] **Auto mode** — autonomous execution (`/yolo` in OpenShark)
- [ ] **Dynamic workflows** (`ultracode`) — orchestrates 10s-100s of background agents
- [x] **Background agents** — `/headless` spawns detached sessions
- [ ] **Git worktree isolation** for background sessions
- [x] **Plugin system** — `~/.config/openshark/hooks/` auto-loaded
- [x] **MCP native support** — stdio + HTTP, auto-discovery
- [x] **Skills** with YAML frontmatter
- [x] **Hooks** — SessionStart, PostToolUse, Stop, MessageDisplay
- [x] **Slash commands** — 40+ commands implemented
- [ ] **Effort levels** — low/medium/high/xhigh/ultracode
- [ ] **Thinking budgets** — configurable per-run
- [x] **Context compaction** — `/compact` with agentic/basic/off modes
- [x] **Vim mode** — full vim keybindings in TUI (`/vim`)
- [x] **Mouse support** — click, scroll, hover (`/mouse`)
- [ ] **Copy-on-select** — clipboard integration
- [ ] **Image paste** — screenshots, drag-and-drop
- [ ] **Voice mode** — push-to-talk
- [ ] **OpenTelemetry** — full metrics, traces, logs
- [ ] **Managed settings** — enterprise policy enforcement
- [x] **Sandbox** — basic permission profiles (`sandbox.rs` exists)
- [ ] **Auto-updater** — built-in update mechanism
- [ ] **Resume** — cross-session, cross-directory, background sessions
- [x] **Checkpoints** — `/checkpoint` and `/restore`
- [x] **Cost tracking** — `/usage` shows per-session breakdown
- [ ] **JSON output** — `claude -p --json` for scripting
- [ ] **Desktop app integration** — menubar notifications
- [ ] **IDE extensions** — VS Code, JetBrains
- [ ] **Remote control** — claude.ai mobile app bridge
- [ ] **OAuth** — Cline, ChatGPT Subscription, OCA
- [x] **Connectors** — Discord, Telegram, Slack, Matrix gateway
- [ ] **Schedules** — cron-like agent scheduling
- [ ] **Hub daemon** — background task management (`--zen`)

---

## 2. CODEX CLI (OpenAI) — v0.136.0

### Core Features
- [x] **Rust-based TUI** — ratatui, markdown rendering, syntax highlighting
- [x] **Streaming** — real-time token streaming
- [x] **MCP support** — native client, stdio + HTTP
- [ ] **App-server protocol** — v2 API for integrations
- [ ] **Remote execution** — exec-server with websockets
- [x] **Sandbox** — basic permission profiles exist
- [x] **Skills** — frontmatter-based, auto-loaded from dirs
- [x] **Hooks** — multiline output rendering
- [ ] **Guardian** — code review agent with cache keys
- [ ] **Multi-agent** — v2 assignment tool
- [ ] **Archive/unarchive** sessions
- [ ] **OSC 8 hyperlinks** — clickable web links in terminal
- [x] **Vim mode** — normal mode editing (`/vim`)
- [ ] **Goal steering** — internal context fragments
- [x] **Permission profiles** — filesystem access control (`sandbox.rs`)
- [ ] **Image generation** — feature-gated extension
- [ ] **Python SDK** — `pip install openai-codex`
- [ ] **Config schema** — JSON schema for validation
- [ ] **Shell completions** — bash, zsh, fish

---

## 3. CLINE — v3.12.0

### Core Features
- [x] **Multi-provider chat** — OpenAI, Anthropic, Google, OpenRouter, local
- [x] **TUI with ratatui** — 24 themes, sidebar, keybindings
- [x] **Streaming responses**
- [x] **Tool system** — 32 tools (fs, git, search, terminal, web, etc.)
- [x] **Memory hierarchy** — session → project → global
- [x] **Vector embeddings** — semantic search
- [x] **Swarm mode** — 8 roles, consensus memory
- [x] **Gateway** — Discord, Telegram, Slack, Matrix
- [x] **MCP client** — native stdio/HTTP
- [x] **Skills system** — YAML-based
- [x] **Slash commands** — 40+ commands
- [x] **Command palette** — fuzzy search
- [x] **Session bookmarks / checkpoints**
- [x] **Inline image display**
- [x] **Streaming markdown renderer**
- [x] **Syntax highlighting**
- [x] **Context compression**
- [x] **Evolution engine**
- [x] **Self-improvement tracking**
- [x] **Setup wizard**
- [x] **Doctor** — auto-repair
- [x] **Security config**
- [x] **Session export**
- [x] **Stats command**
- [x] **Multi-model chat**
- [x] **Plan/Act mode** — toggle between planning and execution
- [x] **Checkpoints** — `/undo` to rewind state
- [ ] **Agent teams** with persistent state
- [x] **Plugin system** with lifecycle hooks
- [ ] **`.clinerules`** project rules
- [ ] **OAuth** authentication
- [x] **Chat connectors** — gateway exists
- [ ] **Cron scheduling**
- [ ] **Zen mode** — background hub daemon (`--zen`)
- [ ] **Yolo mode** — auto-approve all
- [ ] **JSON output** — NDJSON for piping
- [x] **Headless** — CI/CD scripting (`/headless`, `--headless`)
- [ ] **Team workflows** — persistent named teams
- [ ] **Hub daemon** — local task management
- [ ] **Cline Hub** — web app for monitoring sessions

---

## 4. AIDER — v0.86.0

### Core Features
- [x] **Repo map** — `/map` command implemented
- [x] **Architect/Editor mode** — `/architect`, `/editor` toggle
- [x] **Ask mode** — `/ask` read-only Q&A
- [ ] **Context mode** — auto-identify files to edit
- [ ] **Voice-to-code** — speech input
- [ ] **Web scraping** — Playwright-based page ingestion
- [x] **Lint-and-fix** — `/lint` auto-runs linters
- [ ] **Test runner** — auto-run tests on changes
- [x] **Git integration** — auto-commit exists
- [ ] **Copy/paste web chat** — bridge to browser UIs
- [ ] **Image support** — paste images into chat
- [ ] **Multiple edit formats** — diff, udiff, whole, patch, editor-diff
- [x] **Weak model** — config field exists
- [x] **Editor model** — config field exists
- [ ] **Thinking tokens** — reasoning budget control
- [ ] **Reasoning effort** — low/medium/high
- [ ] **Co-authored-by** — git attribution
- [ ] **Shell completions** — bash, zsh
- [ ] **Watch mode** — file watcher for AI comments
- [ ] **Benchmark mode** — systematic evaluation
- [ ] **Analytics** — PostHog integration
- [x] **Model aliases** — convenient shortcuts

---

## 5. CONTINUE — v1.2.22

### Core Features
- [ ] **AI checks in CI** — markdown-based PR checks
- [ ] **Checks as code** — `.continue/checks/` directory
- [ ] **CLI (`cn`)** — headless check runner
- [ ] **VS Code extension** — primary interface
- [ ] **Config as code** — `config.yaml` in repo

---

## 6. OPENSHARK — v1.2.0 (Current)

### What We Have
- [x] Multi-provider chat (OpenAI, Anthropic, Google, OpenRouter, local)
- [x] TUI with ratatui — 24 themes, sidebar, keybindings
- [x] Streaming responses
- [x] Tool system — 32 tools (fs, git, search, terminal, web, etc.)
- [x] Memory hierarchy — session → project → global
- [x] Vector embeddings — semantic search
- [x] Swarm mode — 8 roles, consensus memory
- [x] Gateway — Discord, Telegram, Slack, Matrix
- [x] MCP client — native stdio/HTTP
- [x] Skills system — YAML-based
- [x] Slash commands — 40+ commands
- [x] Command palette — fuzzy search
- [x] Session bookmarks / checkpoints
- [x] Inline image display
- [x] Streaming markdown renderer
- [x] Syntax highlighting
- [x] Context compression
- [x] Evolution engine
- [x] Self-improvement tracking
- [x] Setup wizard
- [x] Doctor — auto-repair
- [x] Security config
- [x] Session export
- [x] Stats command
- [x] Multi-model chat
- [x] Plan/Act mode toggle
- [x] Checkpoints with /undo
- [x] Vim mode in TUI
- [x] Mouse support in TUI
- [x] Context compaction (agentic/basic/off)
- [x] Usage/cost tracking
- [x] Lint-and-fix
- [x] Repo map
- [x] Background sessions (/headless)
- [x] Architect/Editor dual-model mode
- [x] Ask mode
- [x] Weak model / Editor model separation
- [x] Auto-commit
- [x] Hooks system
- [x] Skills auto-discovery

### What We Still Lack

#### TIER 1 — High Impact, Medium Effort (Do First)
- [ ] **Auto/YOLO mode** — auto-approve all tool calls without prompting
- [ ] **Auto-commit with LLM-generated messages** — generate commit message via LLM
- [ ] **Test runner auto-run** — after edits, auto-run tests and report
- [ ] **Effort levels / thinking budgets** — `/effort low|medium|high|xhigh`
- [ ] **Copy-on-select** — clipboard integration
- [ ] **Git worktree isolation** — background sessions use worktrees
- [ ] **OSC 8 hyperlinks** — clickable URLs in terminal
- [ ] **Context mode** — auto-identify files to edit

#### TIER 2 — High Impact, High Effort
- [ ] **Dynamic workflows** — multi-agent orchestration at scale
- [ ] **Sandbox v2** — permission profiles, filesystem isolation
- [ ] **Guardian code review agent** — secondary agent reviews edits
- [ ] **Voice mode** — push-to-talk speech input
- [ ] **Image paste / drag-drop** — paste screenshots into chat
- [ ] **Web scraping** — Playwright integration
- [ ] **Multiple edit formats** — diff, patch, udiff, editor-diff
- [ ] **Archive/unarchive sessions** — save/load session state
- [ ] **Cross-directory resume** — resume from any directory

#### TIER 3 — Medium Impact, Medium Effort
- [ ] **Desktop notifications** — system notifications for completions
- [ ] **JSON/NDJSON output mode** — `openshark -p "prompt" --json`
- [ ] **Team workflows** — persistent named agent teams
- [ ] **Co-authored-by attribution** — git commits tagged
- [ ] **Watch mode** — file watcher triggers agent
- [ ] **AI checks for CI/CD** — `.openshark/checks/` directory
- [ ] **Config JSON schema** — validate config against schema
- [ ] **PostHog / analytics** — opt-in usage analytics

#### TIER 4 — Lower Impact or Niche
- [ ] **IDE extension (VS Code)** — massive effort
- [ ] **OAuth login flow** — web-based auth
- [ ] **Chat connectors (native)** — native bots vs gateway
- [ ] **Cron scheduling** — recurring agent tasks
- [ ] **Hub daemon / zen mode** — background task management
- [ ] **OpenTelemetry** — metrics, traces, logs
- [ ] **Remote control / mobile bridge**
- [ ] **Managed settings / enterprise policies**
- [ ] **Python SDK** — `pip install openshark`
- [ ] **Benchmark mode** — systematic eval
- [ ] **Auto-updater** — built-in update mechanism
- [ ] **Shell completions** — bash, zsh, fish
- [ ] **Goal steering** — internal context fragments

---

## IMPLEMENTATION ROADMAP

See `docs/ROADMAP.md` for the master implementation plan.

### Recommended Order

1. **Tier 1** — All 8 features. These are table-stakes for a competitive harness.
2. **Tier 2** — Pick 3-4 based on user feedback. Guardian agent and multiple edit formats are highest value.
3. **Tier 3** — Nice-to-haves. JSON output and watch mode are easiest wins.
4. **Tier 4** — Only if explicitly requested. IDE extension is a 6-month project.

---

*Last updated: 2026-06-03 after v1.2 Tier 1-4 parity push*
