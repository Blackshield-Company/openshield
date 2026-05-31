# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-06-01

### Added

- **Swarm Mode** — Multi-agent orchestration with 8 built-in roles (Architect, Implementer, Reviewer, Tester, DevOps, Security, Documentation, PM). Consensus memory, autonomous event-driven loops, real LLM integration per agent with isolated context. `openshark swarm init/start/stop/status` CLI commands.
- **Context Compression** — Token-aware context compression with semantic summarization. Keeps long conversations within model context limits automatically.
- **Evolution Engine** — Self-adaptive behavior engine that tracks tool outcomes, model performance, and session quality to evolve routing weights and behavior over time.
- **Matrix Gateway** — Full sync loop scaffold with `MatrixReplySender`, homeserver validation, and unified router integration.
- **Slack Gateway** — Socket Mode scaffold with `SlackReplySender`, ready event emission, and full Socket Mode connection structure.
- **Swarm CLI** — Terminal commands for multi-agent orchestration: `init`, `start`, `stop`, `status`.

### Changed

- **Version bump** — 0.4.0 → 1.0.0 (production-ready release).
- **Test count** — 246 → 337 comprehensive tests across all modules.

### Fixed

- **Test compilation errors** — Added missing `context_compression` field to all test Config struct initializers.

## [0.4.0] - 2026-05-30

### Added

- **Multi-model comparison overlay** — `Ctrl+V` toggles a 90%×85% popup showing primary + all secondary model responses with navigation (↑/↓), model names, latency, and token counts.
- **Multi-model response attachment** — Secondary responses attach to the primary assistant message instead of creating truncated system messages.
- **Chat area multi-model indicator** — "📊 3 alternate responses — Ctrl+V to compare" appears on assistant messages with secondary responses.
- **Multi-platform gateway reply paths** — Telegram, Slack, and Matrix gateways now send responses back to their respective platforms (was previously log-only stubs).
- **Telegram reply sender** — `TelegramReplySender` with `Bot` instance; messages chunked at Telegram's 4096 char limit.
- **Slack Socket Mode scaffold** — Real connection structure with `SlackReplySender`; emits `Ready` event; full Socket Mode TODO documented.
- **Matrix sync loop scaffold** — Real connection structure with `MatrixReplySender`; validates homeserver, user_id, access_token config; full sync loop TODO documented.
- **Unified router reply wiring** — All three platforms now pass `ReplySender` to `UnifiedRouter`, which spawns reply tasks that actually call platform APIs.

### Changed

- **Version bump** — 0.1.0 → 0.4.0 (reflecting substantial feature maturity).

### Fixed

- **Telegram responses** — No longer "fire and forget"; replies stream back to the originating chat.

## [0.3.0] - 2026-05-30

### Added

- **Security architecture** — 4-layer security: sandbox, identity, PII, guardrails. Wired into all 5 tool execution paths.
- **Autonomous mode toggle** — `Ctrl+A` switches between safe and full-send security modes.
- **Personalized chat names** — Configurable `user_name` and `agent.display_name` in TUI.
- **Natural language control words** — Pre-filter stop/wait/continue/cancel/go before hitting the model API.
- **24 preset themes** — Synthwave84 default, Omarchy stock, light/dark variants. `Ctrl+T` cycling.
- **Native MCP client** — stdio + SSE transport, JSON-RPC 2.0, tool discovery/execution.
- **Multi-platform gateway** — Discord ✅, Telegram ✅, Slack 🟡, Matrix 🟡.
- **Optional multi-model mode** — Off by default, toggleable at runtime via `/multi` or `!multi`.

### Changed

- **MAX_ITERATIONS** — 88 (was 888 in early dev).
- **Hermes runtime dependency removed** — Setup/config transfer preserved, but no runtime bridges.

### Fixed

- **128 compiler warnings** → 0. Systematic dead code cleanup.

## [0.2.0] - 2026-05-29

### Added

- **Discord gateway** — Native serenity 0.12 bot with 15 slash commands, free-form chat, keyword commands, memory recall.
- **Skills system** — YAML frontmatter + markdown, trigger-based auto-load, 5 built-in skills.
- **Agent identity** — Config-based name, personality, emoji, TUI branding.
- **Streaming TUI** — True streaming responses, async background tasks, responsive input.

## [0.1.0] - 2026-05-28

### Added

- **Core engine** — Chat, SQLite memory, basic tools, provider routing.
- **TUI interface** — ratatui-based, keyboard-driven.
- **Tool system** — File system, git, search, terminal, edit tools.
- **Memory hierarchy** — Session → Project → Global layers with embeddings.
