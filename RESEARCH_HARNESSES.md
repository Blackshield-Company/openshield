# AI Coding Harness Research — Feature Matrix

Research conducted: 2026-06-02
Harnesses analyzed: Claude Code, Codex CLI, Cline, Aider, Continue, OpenShark (self)

---

## 1. CLAUDE CODE (Anthropic) — v2.1.161

### Core Features
- **Agentic coding** with natural language commands
- **Plan/Act mode toggle** — plan explores, act executes
- **Auto mode** — autonomous execution with safety classifier
- **Dynamic workflows** (`ultracode`) — orchestrates 10s-100s of background agents
- **Background agents** (`claude agents`) — attach/detach, persistent sessions
- **Git worktree isolation** for background sessions
- **Subagent spawning** with worktree isolation
- **Plugin system** — `.claude/skills` auto-loaded, marketplace
- **MCP native support** — stdio + SSE, auto-discovery
- **Skills** with frontmatter (disallowed-tools, effort, context)
- **Hooks** — SessionStart, PostToolUse, Stop, MessageDisplay
- **Slash commands** — 40+ commands: /model, /plugin, /mcp, /doctor, /goal, /effort, /workflows, /code-review, /simplify, /context, /clear, /compact, /usage, /settings, /rename, /diff, /branch, /add-dir, /bg, /btw, /chrome, /remote-control, /terminal-setup, /vim, /theme, /scroll-speed, /copy, /export, /feedback, /loop, /tasks, /tag, /rules, /memory, /fast, /resume, /fork, /rewind, /archive, /autofix-pr, /commit-push-pr, /ultraplan, /ultrareview
- **Effort levels** — low/medium/high/xhigh/ultracode
- **Thinking budgets** — configurable per-run
- **Context compaction** — agentic/basic/off modes
- **Vim mode** — full vim keybindings in TUI
- **Mouse support** — click, scroll, hover
- **Copy-on-select** — clipboard integration
- **Image paste** — screenshots, drag-and-drop
- **Voice mode** — push-to-talk
- **OpenTelemetry** — full metrics, traces, logs
- **Managed settings** — enterprise policy enforcement
- **Sandbox** — permission profiles, allow/deny lists
- **Auto-updater** — built-in update mechanism
- **Resume** — cross-session, cross-directory, background sessions
- **Checkpoints** — /undo to rewind workspace state
- **Cost tracking** — per-session, per-tool, per-MCP usage breakdown
- **JSON output** — `claude -p --json` for scripting
- **Desktop app integration** — menubar notifications
- **IDE extensions** — VS Code, JetBrains
- **Remote control** — claude.ai mobile app bridge
- **OAuth** — Cline, ChatGPT Subscription, OCA
- **Connectors** — Telegram, Slack, Google Chat, WhatsApp, Linear
- **Schedules** — cron-like agent scheduling
- **Hub daemon** — background task management (`--zen`)

### What OpenShark Lacks
- [ ] Plan/Act mode toggle
- [ ] Auto mode with safety classifier
- [ ] Dynamic workflows (multi-agent orchestration at scale)
- [ ] Background agent sessions with attach/detach
- [ ] Git worktree isolation
- [ ] Plugin marketplace + auto-discovery
- [ ] Hooks system (SessionStart, PostToolUse, etc.)
- [ ] Effort levels / thinking budgets
- [ ] Context compaction (agentic summarization)
- [ ] Vim mode in TUI
- [ ] Mouse support in TUI
- [ ] Copy-on-select clipboard
- [ ] Image paste / drag-drop
- [ ] Voice mode
- [ ] OpenTelemetry integration
- [ ] Managed settings / enterprise policies
- [ ] Sandbox permission profiles
- [ ] Auto-updater
- [ ] Cross-directory resume
- [ ] Cost tracking / usage breakdown
- [ ] JSON output mode for scripting
- [ ] Desktop notifications
- [ ] IDE extensions
- [ ] Remote control / mobile bridge
- [ ] OAuth login flow
- [ ] Chat connectors (Telegram, Slack, etc.)
- [ ] Cron scheduling
- [ ] Hub daemon / zen mode

---

## 2. CODEX CLI (OpenAI) — v0.136.0

### Core Features
- **Rust-based TUI** — ratatui, markdown rendering, syntax highlighting
- **Streaming** — real-time token streaming
- **MCP support** — native client, stdio + SSE
- **App-server protocol** — v2 API for integrations
- **Remote execution** — exec-server with websockets
- **Sandbox** — seatbelt (macOS), bwrap (Linux), Windows sandbox
- **Skills** — frontmatter-based, auto-loaded from dirs
- **Hooks** — multiline output rendering
- **Guardian** — code review agent with cache keys
- **Multi-agent** — v2 assignment tool
- **Archive/unarchive** sessions
- **OSC 8 hyperlinks** — clickable web links in terminal
- **Vim mode** — normal mode editing
- **Goal steering** — internal context fragments
- **Permission profiles** — filesystem access control
- **Image generation** — feature-gated extension
- **Python SDK** — `pip install openai-codex`
- **Config schema** — JSON schema for validation
- **Shell completions** — bash, zsh, fish

### What OpenShark Lacks
- [ ] Rust TUI with ratatui (OpenShark has custom TUI)
- [ ] App-server protocol for integrations
- [ ] Remote exec-server
- [ ] Sandbox (seatbelt/bwrap/Windows)
- [ ] Guardian code review agent
- [ ] Multi-agent v2 orchestration
- [ ] Archive/unarchive sessions
- [ ] OSC 8 terminal hyperlinks
- [ ] Goal steering with context fragments
- [ ] Permission profiles for filesystem
- [ ] Image generation pipeline
- [ ] Python SDK
- [ ] Config JSON schema

---

## 3. CLINE — v3.86.2

### Core Features
- **Multi-surface** — CLI, VS Code extension, JetBrains plugin, Kanban board
- **SDK** — `@cline/sdk` for custom agents
- **Plan/Act mode** — toggle between planning and execution
- **Checkpoints** — /undo to rewind state
- **Sub-agent spawning** — agent teams
- **MCP servers** — native support, `cline mcp` CLI
- **Plugins** — custom tools, lifecycle hooks
- **Rules** — `.clinerules` project-specific guidance
- **Skills** — load specific rules when needed
- **OAuth** — Cline, ChatGPT, OCA
- **Connectors** — Telegram, Slack, Google Chat, WhatsApp, Linear
- **Schedules** — cron + event-driven
- **Zen mode** — background hub daemon (`--zen`)
- **Yolo mode** — auto-approve all
- **JSON output** — NDJSON for piping
- **Headless** — CI/CD scripting
- **Thinking budgets** — configurable
- **Context compaction** — agentic/basic/off
- **Team workflows** — persistent named teams
- **Hub daemon** — local task management
- **Cline Hub** — web app for monitoring sessions

### What OpenShark Lacks
- [ ] Multi-surface (IDE extensions, web UI)
- [ ] SDK for third-party agents
- [ ] Plan/Act mode toggle
- [ ] Checkpoints with /undo
- [ ] Agent teams with persistent state
- [ ] Plugin system with lifecycle hooks
- [ ] `.clinerules` project rules
- [ ] OAuth authentication
- [ ] Chat connectors
- [ ] Cron scheduling
- [ ] Zen/background hub mode
- [ ] NDJSON output
- [ ] Headless CI/CD mode
- [ ] Team workflows
- [ ] Cline Hub web monitoring

---

## 4. AIDER — v0.86.0

### Core Features
- **Repo map** — tree-sitter based codebase map for context
- **Architect/Editor mode** — two-model workflow
- **Ask mode** — questions without editing
- **Context mode** — auto-identify files to edit
- **Voice-to-code** — speech input
- **Web scraping** — Playwright-based page ingestion
- **Lint-and-fix** — auto-run linters, apply fixes
- **Test runner** — auto-run tests on changes
- **Git integration** — auto-commit with sensible messages
- **Copy/paste web chat** — bridge to browser UIs
- **Image support** — paste images into chat
- **Multiple edit formats** — diff, udiff, whole, patch, editor-diff
- **Weak model** — separate model for simple tasks
- **Editor model** — separate model for edits
- **Thinking tokens** — reasoning budget control
- **Reasoning effort** — low/medium/high
- **Co-authored-by** — git attribution
- **Shell completions** — bash, zsh
- **Watch mode** — file watcher for AI comments
- **Benchmark mode** — systematic evaluation
- **Analytics** — PostHog integration
- **Model aliases** — convenient shortcuts

### What OpenShark Lacks
- [ ] Repo map (tree-sitter codebase analysis)
- [ ] Architect/Editor dual-model mode
- [ ] Ask mode (read-only Q&A)
- [ ] Context mode (auto file identification)
- [ ] Voice-to-code
- [ ] Web scraping with Playwright
- [ ] Lint-and-fix loop
- [ ] Auto test runner
- [ ] Auto-commit with generated messages
- [ ] Web chat bridge
- [ ] Multiple edit formats (diff, patch, etc.)
- [ ] Weak model / Editor model separation
- [ ] Thinking tokens / reasoning effort
- [ ] Co-authored-by attribution
- [ ] Watch mode for AI comments
- [ ] Benchmark mode
- [ ] PostHog analytics

---

## 5. CONTINUE — v1.2.22

### Core Features
- **AI checks in CI** — markdown-based PR checks
- **Checks as code** — `.continue/checks/` directory
- **CLI (`cn`)** — headless check runner
- **VS Code extension** — primary interface
- **Config as code** — `config.yaml` in repo

### What OpenShark Lacks
- [ ] AI checks for CI/CD
- [ ] Checks-as-code system
- [ ] Headless check runner
- [ ] VS Code extension
- [ ] Repo-level config.yaml

---

## 6. OPENSHARK — v1.1.0 (Current)

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
- [x] Slash commands — partial implementation
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

---

## PRIORITY MATRIX: What to Implement

### TIER 1 — High Impact, Medium Effort (Do First)
1. **Plan/Act mode toggle** — Claude Code & Cline have this. Critical for agent control.
2. **Background sessions** — `claude agents` style attach/detach. Huge UX win.
3. **Repo map** — Aider's killer feature for large codebases. Tree-sitter integration.
4. **Auto-commit with generated messages** — Aider's git integration is best-in-class.
5. **Lint-and-fix loop** — Aider runs linter after edits, auto-fixes. Essential for code quality.
6. **Context compaction** — agentic summarization. Claude Code has three modes.
7. **Checkpoints / undo** — rewind workspace state. Cline & Claude Code both have this.
8. **Vim mode** — essential for terminal power users. Codex & Claude Code have it.

### TIER 2 — High Impact, High Effort (Plan Carefully)
9. **Dynamic workflows** — multi-agent orchestration at scale. Claude Code's ultracode.
10. **Sandbox** — permission profiles, filesystem isolation. Codex has seatbelt/bwrap.
11. **Plugin system** — marketplace, auto-discovery. Claude Code's `.claude/skills`.
12. **Hooks system** — SessionStart, PostToolUse, etc. Claude Code's extensibility.
13. **IDE extension** — VS Code integration. Cline's multi-surface approach.
14. **Chat connectors** — Telegram, Slack bridges. Cline & Claude Code both have.
15. **Voice mode** — speech input. Aider has this.
16. **Web scraping** — Playwright integration. Aider's web page ingestion.

### TIER 3 — Medium Impact, Medium Effort (Nice to Have)
17. **Mouse support** — click, scroll in TUI. Claude Code has this.
18. **Copy-on-select** — clipboard integration. Claude Code feature.
19. **Image paste / drag-drop** — Claude Code & Aider support.
20. **Cost tracking** — per-session usage breakdown. Claude Code has /usage.
21. **JSON output mode** — NDJSON for scripting. Cline & Claude Code have.
22. **Headless CI/CD mode** — non-interactive execution. Cline's `--yolo`.
23. **OAuth login** — streamlined auth. Cline & Claude Code have.
24. **Auto-updater** — built-in update mechanism. Claude Code has this.
25. **Archive/unarchive** — session management. Codex recently added.
26. **OSC 8 hyperlinks** — clickable links in terminal. Codex has this.
27. **Team workflows** — persistent named teams. Cline has this.
28. **Cron scheduling** — recurring agent tasks. Cline & Claude Code have.
29. **Hub daemon / zen mode** — background task management. Cline has this.
30. **OpenTelemetry** — metrics and tracing. Claude Code has this.

### TIER 4 — Lower Impact or Niche
31. **Managed settings / enterprise policies**
32. **Remote control / mobile bridge**
33. **Desktop notifications**
34. **Co-authored-by attribution**
35. **Watch mode for AI comments**
36. **Benchmark mode**
37. **PostHog analytics**
38. **AI checks for CI/CD** (Continue's niche)
39. **Python SDK**
40. **Config JSON schema**

---

## IMPLEMENTATION RECOMMENDATION

Start with **Tier 1** in this order:
1. Plan/Act mode (fastest win — toggle in TUI)
2. Checkpoints / undo (leverage existing bookmark system)
3. Auto-commit (extend existing git tool)
4. Vim mode (ratatui supports this)
5. Lint-and-fix loop (extend existing tool system)
6. Context compaction (build on existing compression)
7. Repo map (largest effort — tree-sitter integration)
8. Background sessions (architectural change)

Then move to Tier 2 based on user feedback.
