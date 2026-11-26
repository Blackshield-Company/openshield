# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- **config.toml no longer collects resolved API keys** — the first-run config copied any `OPENAI_API_KEY`, `KIMI_API_KEY`, `ANTHROPIC_API_KEY` and so on from the environment into `config.toml` as plain text, and every later save (`/sprite`, `/motion`, `openshield setup`, the doctor's Config fix) wrote back whatever keys and bot tokens had been resolved from the environment or `*.env` files. Saves now write the original `${VAR}` text, and config.toml (and its `.bak`) is written owner-only (0600) on Unix.
- **Legacy TUI HTTP gateway requires the bearer token and is opt-in** — the 127.0.0.1:9876 API (`openshield server`, and the listener the TUI used to start automatically) now requires the same `Authorization: Bearer` token as `openshield serve` (`[desktop] token_path`, usually `~/.config/openshield/desktop.token`). Non-loopback binds need `[desktop] lan_mode = true`. The TUI no longer starts this gateway by default; enable it with `openshield --tui-http` or `[desktop] tui_http = true`. Desktop and Pocket should keep using `openshield serve` on port 9310.

### Added

- **Persisted approvals, audit log, and richer session export** — approval decisions (including the 120s timeout-deny) and security audit rows are written to `memory.db` (`approvals` and `audit_log` tables) so they survive process exit. `SessionExport` now includes `tool_calls`, persisted approvals, audit rows, and an estimated USD cost (blended from the model's per-1k input/output rates and labelled `cost_is_estimate`). Older export files still load.
- **Cargo workspace and built-in modules** — the repo is a Cargo workspace with `crates/openshield-sdk` (module contract: `[modules]` config, hook types, `BuiltinModule`) and `extensions/replay`. `config.toml` accepts a `[modules.<id>]` table (`replay`, `eval`, `armory`, `guard`; default enabled). Cargo features `replay` (default on), `eval`, `armory` and `guard` (the last three are reserved no-ops until those crates land). `openshield replay status`, `GET /api/v1/replay/status`, and `openshield doctor` report compiled/enabled state. The `openshield` package name and library path are unchanged, so desktop and Pocket still talk to `openshield serve` and in-process `openshield::serve_in_process` the same way.
- **xAI Grok 4.7 out of the box** — the default config now has an `xai` provider (`https://api.x.ai/v1`) with `grok-4.7` (500,000-token context, $2 / $6 per 1M tokens). It uses `XAI_API_KEY` (or `XAI_API_KEY=` in `~/.config/openshield/xai.env`) when set, and the SuperGrok token from `openshield auth xai` otherwise. Before this, `XAI_API_KEY` was documented but no provider read it, and `openshield auth xai` stored a token that nothing used unless the provider was written by hand.
- **`reasoning_effort` for Grok** — `effort_level` (`/effort low|medium|high|xhigh`) is now sent as `reasoning_effort` to models that declare the `reasoning_effort` capability (the default `grok-4.7` does). Other models never receive the field. Before this, `/effort` only changed a line in the system prompt.
- **`POST /api/v1/doctor/fix`** — runs `openshield doctor --fix` and returns the post-fix report plus `fixes_applied`, for the Desktop doctor panel.


- **Embedded server requires the bearer token** — `serve_in_process` (the Android/Pocket in-app server) no longer runs with `auth: None`. Every REST request, and every WebSocket upgrade, must present the token from `config_dir/desktop.token` (created with mode 0600 on first start; read it with `openshield::embedded_token`) as `Authorization: Bearer`, or `?token=` on `/ws/` upgrades. Non-loopback binds are refused unless `[desktop] lan_mode = true`. `serve_in_process_with_token` accepts a token supplied by the host.
- **Sandbox path checks cover traversal and compact JSON** — tool arguments such as `{"path":"/etc/passwd"}`, nested JSON values, `--file=/x`, `../x` and `..` are now checked against the working area. `..` is resolved before the allow check, so `<workdir>/missing/../../etc/passwd` no longer passes as inside the working directory. `~` is expanded before the check.
- **Dependency advisories** — serenity now uses its native-tls backend, which removes `rustls` 0.22 / `rustls-webpki` 0.102.8 (GHSA-82j2-j2ch-gfr8 and related advisories) from the tree. The remaining `rustls-webpki` is 0.103.17 (RUSTSEC-2026-0382). `wayland-scanner` 0.31.11 brings `quick-xml` 0.41.0 (RUSTSEC-2026-0194, RUSTSEC-2026-0195).
- **Source and release builds need OpenSSL 3** — `discord` is a default feature and now uses native TLS, so source builds on Linux need the OpenSSL development headers and pkg-config (`libssl-dev` and `pkg-config` on Debian/Ubuntu). The Linux release binaries now link `libssl.so.3`.

### Fixed

- **An unknown model no longer goes to a random provider** — when `--model` / `default_model` / a WS or REST `model` named a model that no provider lists, the harness, `openshield chat`, `/api/v1/chat`, `/api/v1/agent` and the WS agent picked "the first" provider of a HashMap, which is effectively random. The prompt could go to a provider nobody chose, or fail with an unrelated `OPENROUTER_API_KEY is undefined`. It is now an error that lists the known models. A config with exactly one provider still sends unlisted model names to it (proxies, Ollama).
- **`/ws/v1/chat` sends `turn_done` once, at the end of the turn** — it was sent after the first model response, before any tool ran, so clients treated the turn as finished while tools, approvals and the final answer were still streaming. The full answer was also re-sent as one extra `token` after `turn_done`. `turn_done` now comes last, its `ms` is the real turn duration (it was always 0), and the final text is only sent again if it was never streamed.
- **`headless --json` stdout is pure NDJSON** — the `🛡 OpenShield Headless Mode` banner went to stdout ahead of the JSON events, and with `--yolo` / `--autonomous` the self-improvement report printed its tables there too, so line-by-line JSON parsing failed. In `--json` mode the banner goes to stderr and the report is skipped.
- **`openshield doctor --fix` works on a broken config** — `doctor` used to load `config.toml` before running, so an invalid or empty config made `doctor --fix` fail with the same parse error it is meant to repair. It now runs before the config is loaded. The Config check also deserializes into the real config type, so valid TOML of the wrong shape (for example `default_model = 5`) is reported instead of passing as healthy. Other commands that hit a bad config now point at `openshield doctor --fix`.
- **Package author** — `Cargo.toml` `authors` uses the GitHub noreply address.
- **CI and installer** — three `clippy::needless_bool` errors in `src/providers/mod.rs` are fixed, the test runner tool checks the command before validating paths (and checks blocked prefixes for paths that do not exist), and `install.sh` points at `Blackshield-Company/openshield`.

## [1.2.2] - 2026-10-20

### Fixed

- **xAI/OpenAI streaming errors are visible** — API errors and non-SSE response bodies from xAI or OpenAI-compatible endpoints now surface as errors instead of producing an empty reply. The shared stream parser has tests that run without an xAI login.

## [1.2.1] - 2026-10-17

### Added

- **Reduced motion for the knight** — The terminal knight can show a still frame (the first frame of its current state) instead of animating. Turn it on with `reduced_motion = true` in `config.toml`, by setting `NO_MOTION` or `REDUCE_MOTION` (any value except `0`, `false`, `no` or `off`), or at runtime with `/motion off` (`/motion on` animates again, `/motion toggle` flips it, `/motion` shows the setting; the choice is saved to config). Switching it on mid-animation settles the knight on its still frame. With this, every OpenShield app respects reduced motion: the desktop app and the browser extension already show a still knight under `prefers-reduced-motion`.

## [1.2.0] - 2026-10-16

### Added

- **Web terminal demo** — `openshield demo` serves the full TUI in a browser terminal against a built-in mock model, with an isolated config and no API keys. `openshield mock-provider` runs the mock OpenAI-compatible API on its own.

### Changed

- **Black Steel look** — New brand system in `docs/brand` (shield, wordmark, tokens). The TUI theme and launch screen use the Black Steel palette and a flat shield with a plain Greek cross.
- **Launch screen** — No more "Press any key to start": the first typed character goes straight into the prompt.
- **Default sprite is now `none`** — The launch screen shows the Black Steel shield beside the wordmark by default. The knight is still available: set `sprite = "blackshield-mercenary"` in `config.toml` or run `/sprite blackshield-mercenary`. Configs that already set `sprite` explicitly keep their choice.
- **Knight sprite redrawn** — `blackshield-mercenary` is a new Black Steel knight: great helm with a blood plume, pauldrons, crimson cloak, longsword and a heater shield with a plain Greek cross, drawn in truecolor half-blocks (32×14 cells, two frames). Regenerate it with `scripts/knight-sprite.py`; the old `scripts/petdex-to-ans.py` converter is removed.
- **Sprite animation states** — Sprite files can group frames under `---state:<name>---` markers (`idle`, `thinking`, `working`, `success`, `error`); files without markers still load as a single idle animation. The knight ships all five states, and the splash picks the state from agent activity.

### Security

- **Dependencies** — Updated vulnerable crates in `Cargo.lock`, including rustls 0.23.46.

## [1.1.1] - 2026-10-14

### Security

- **Serve auth** — Bearer token is now required on every `openshield serve` request, including loopback binds. CORS is an allowlist (Tauri and localhost origins, plus `[desktop] allowed_origins`). `Host` is validated against loopback names, the bind address, the LAN address on wildcard binds, and `[desktop] allowed_hosts` (DNS rebinding). Token comparison is constant-time. `POST /api/v1/pair` requires the master token, is limited to 5 requests per 60s, and stores at most 32 pairing tokens.

### Changed

- **Clients must send the token** — Desktop reads `~/.config/openshield/desktop.token`. HTTP requests use `Authorization: Bearer`. WebSocket upgrades may pass `?token=` because browsers cannot set headers on the handshake.

## [1.1.0] - 2026-10-11

### Release engineering

- **Prebuilt binaries** — Release workflow builds Linux x86_64/aarch64, macOS x86_64/arm64 and Windows x86_64 archives on every `v*` tag, with per-asset `.sha256` files and an aggregate `SHA256SUMS`.
- **Installer** — `install.sh` now installs from `synthalorian/openshield`: downloads the release binary for your OS/arch, verifies the checksum, and falls back to `cargo install --git`. Supports `OPENSHIELD_VERSION`, `BIN_DIR`, `OPENSHIELD_FROM_SOURCE` and `DRY_RUN`.
- **CI** — rustfmt, clippy (`-D warnings`), shellcheck and tests on Ubuntu, macOS and Windows.
- **Tests** — Integration tests for the agent plan/execute/verify loop against a mocked OpenAI-compatible provider, the tool approval gate, and the SQLite memory store and search.
- **SECURITY.md** — Vulnerability reporting policy.

### Fixed

- **License metadata** — README badge and `Cargo.toml` now say Apache-2.0, matching `LICENSE` (previously mislabelled MIT).
- **Repository links** — README, `Cargo.toml` and the installer point at `synthalorian/openshield`.


### Added

- **Command Palette** — Fuzzy-searchable command overlay triggered by `Ctrl+P` or typing `/` in input. 16 commands with keyboard navigation (↑/↓, Enter to execute, Esc to close, Backspace to filter).
- **Session Bookmarks / Checkpoints** — Save and restore named session checkpoints. `Ctrl+Shift+B` toggles the bookmark manager. Ctrl+N to create, Enter to load, Esc to close. Persistent per-session JSON storage.
- **Inline Image Display** — Rich metadata indicators for pasted/attached images: format (PNG/JPEG/GIF/WebP/BMP), dimensions, file size. ASCII art placeholder boxes rendered in chat area.
- **Streaming Markdown Renderer** — Inline markdown parsing for assistant messages: **bold**, *italic*, `code`, [links](url), ~~strikethrough~~. Applied to all non-code-block content in real-time.
- **Connection Pooling** — `reqwest` client with `pool_max_idle_per_host(10)`, TCP keepalive, 15s connect timeout, HTTP/2 support. Reduces connection overhead for repeated API calls.
- **Async Cache Persistence** — Cache `set()` now spawns disk write via `tokio::spawn`, eliminating synchronous fs blocking on the hot path.
- **Cache Key Optimization** — Eliminated double JSON serialization in `chat_stream` by reusing `build_chat_body()` output for cache keys.

### Changed

- **Compiler warnings** — 40 → 0. Systematic cleanup across all modules.
- **Test count** — 337 → 363 tests (added image_display dimension detection, format indicator, ASCII placeholder, human_size tests).

### Fixed

- **Test compilation** — Added missing `keybindings` field to all test `Config` struct initializers (config, router, self_improve modules).

## [1.0.0] - 2026-06-01

### Added

- **Swarm Mode** — Multi-agent orchestration with 8 built-in roles (Architect, Implementer, Reviewer, Tester, DevOps, Security, Documentation, PM). Consensus memory, autonomous event-driven loops, real LLM integration per agent with isolated context. `openshield swarm init/start/stop/status` CLI commands.
- **Swarm Real-Time Streaming** — Per-agent chunk streaming with role-colored headers in the TUI. Agent internal monologue visible in real-time, filtered for persona-preamble noise. Broadcast channel architecture for live TUI updates.
- **Swarm Persona Filter** — Strips "I am the X agent" self-convincing preamble from agent responses. 500+ pattern coverage across all roles, applied per-chunk and to final results.
- **Swarm Inspector Sidebar** — Fourth sidebar tab (Ctrl+S) showing all active agents with status, content preview, and expandable tool results. Enter to toggle expansion. 📄 icon when code detected.
- **Swarm Staggered Starts** — Agents start with 2s delays between them to avoid provider contention. Timeout bumped to 180s for parallel agent execution.
- **24 Native Capability Tools** — Zero-external-dependency tool suite: web search, browser automation, X/Twitter search, vision, image generation, video, TTS, memory, session search, todo, cronjob, skills, messaging, Home Assistant, Spotify, Yuanbao, computer use, Mixture of Agents, delegation, clarifying questions, and Python code execution. 32 total tools.
- **Syntax Highlighting** — Full syntax highlighting for code blocks in TUI and swarm streaming. Supports Rust, Python, JavaScript/TypeScript, JSON, TOML, YAML, Bash. Keywords (magenta), types (cyan), strings (green), numbers (yellow), comments (gray italic). Code blocks render with `┌─ code ─` / `└─────────` borders.
- **Context Compression** — Token-aware context compression with semantic summarization. Keeps long conversations within model context limits automatically.
- **Evolution Engine** — Self-adaptive behavior engine that tracks tool outcomes, model performance, and session quality to evolve routing weights and behavior over time.
- **Matrix Gateway** — Full sync loop scaffold with `MatrixReplySender`, homeserver validation, and unified router integration.
- **Slack Gateway** — Socket Mode scaffold with `SlackReplySender`, ready event emission, and full Socket Mode connection structure.
- **Swarm CLI** — Terminal commands for multi-agent orchestration: `init`, `start`, `stop`, `status`.
- **`openshield tools` CLI** — `openshield tools list` shows all native and capability tools with descriptions.

### Changed

- **Version bump** — 0.4.0 → 1.0.0 (production-ready release).
- **Test count** — 246 → 337 comprehensive tests across all modules.
- **Token estimation** — Switched from word-count to char/4 heuristic for more accurate context usage tracking.
- **Swarm config gate removed** — `openshield swarm init` no longer requires `enabled = true` in config. Swarm is always available.

### Fixed

- **Test compilation errors** — Added missing `context_compression` field to all test Config struct initializers.
- **TUI cursor positioning** — Word-wrap cursor position now matches Paragraph widget rendering exactly. No more drift on wrapped lines.
- **Swarm config reload** — `/swarm init` now reloads config from disk so runtime edits take effect immediately.

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
- **3 TUI themes** — Blackshield default, Steel Blue, and High Contrast. `Ctrl+T` cycling.
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
