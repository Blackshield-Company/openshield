# OpenShield Desktop — Design Document

> The shield gets a face. ⚫🦞
> Working title: **OpenShield Desktop** (GUI for the OpenShield CLI harness) + **OpenShield Pocket** (mobile companion).
> Made by synth with Blackclaw.

Status: DESIGN — 2026-09-15
Target: v0.1.0 "Pincer" release

---

## 1. Concept

OpenClaw has its GUI companions; OpenShield deserves its own pincers.

**OpenShield Desktop** is a native desktop app that drives the existing
`openshield` CLI harness — sessions, streaming chat, tool calls, approvals,
swarm, memory, doctor — without a terminal.

**OpenShield Pocket** is the mobile port (same codebase) that pairs with the
desktop app over LAN, so synth can watch runs, approve risky tool calls, and
steer sessions from the couch (or the praise-team green room).

Key insight: **no new backend is required.** OpenShield already ships
`src/api/` (axum REST + WebSocket behind the `web-api` feature) and a
`harness/` engine with an event system. The GUI is a *client* of the harness
itself. The desktop app is pincer one; the phone is pincer two. Both grip the
same `openshield serve` process.

## 2. System Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                     DESKTOP MACHINE (CachyOS)                │
│                                                              │
│  ┌──────────────────┐      ┌──────────────────────────────┐  │
│  │ OpenShield       │      │ openshield serve             │  │
│  │ Desktop (Flutter)│─────▶│ (axum REST + WS, existing    │  │
│  │ lib/             │ WS+  │  src/api/, extended)         │  │
│  └──────────────────┘ HTTP │  127.0.0.1:9310 (default)    │  │
│                            │  0.0.0.0:9310  (LAN mode)    │  │
│                            └──────────┬───────────────────┘  │
│                                       │                      │
│                            ┌──────────▼───────────────────┐  │
│                            │ OpenShield engine (in-proc)  │  │
│                            │  harness/ agent/ memory/     │  │
│                            │  tools/ providers/ swarm/    │  │
│                            │  security engine (L1–L4)     │  │
│                            └──────────────────────────────┘  │
└──────────────────────────────────────┬───────────────────────┘
                                       │ LAN (token auth)
                              ┌────────▼─────────┐
                              │ OpenShield       │
                              │ Pocket (Flutter) │
                              │ Pixel 8a first   │
                              └──────────────────┘
```

### Why in-proc `openshield serve` instead of a separate Rust backend

Hermes Wingman needed a Rust backend because Hermes is a Python process with
no embeddable API surface. OpenShield IS the Rust process. Spawning
`openshield headless` per request loses streaming, session state, and the
security gate. Embedding via the existing `src/api/` module keeps:

- WS streaming (`/ws/v1/chat`, `/ws/v1/agent`) — already implemented
- SQLite session memory — one store, one owner, no split-brain
- The L1–L4 security engine in the loop for every remote action
- TUI, Discord gateway, and desktop app all sharing the same sessions DB

## 3. Component Breakdown

### 3.1 `openshield serve` — the spine (Rust, extends `src/api/`)

New CLI subcommand + hardened API surface. Existing endpoints stay; additions:

| Method | Path | Purpose |
|--------|------|---------|
| GET    | /api/v1/health | Health + version + active model + theme (exists; extend payload) |
| GET    | /api/v1/models | Providers + models parsed from config.toml (exact-match router names) |
| POST   | /api/v1/models/select | Switch default/weak/architect/editor model for the session |
| WS     | /ws/v1/chat | Streaming chat (exists — add tool-call and approval event types) |
| WS     | /ws/v1/agent | Agent run events (exists — add guardrail approval events) |
| GET    | /api/v1/approvals | Pending security-gate approvals (High/Critical risk) |
| POST   | /api/v1/approvals/:id | approve / reject / approve-always |
| GET    | /api/v1/memory/search?q= | Memory browser |
| GET    | /api/v1/skills | Skill registry listing + toggle |
| GET    | /api/v1/doctor | Doctor report (reuse `openshield doctor` logic) |
| GET    | /api/v1/swarm | Swarm status (engine state, agents, consensus doc) |
| POST   | /api/v1/pair | Issue a short-lived pairing token + payload for QR |
| GET    | /api/v1/perf | Per-session perf (tokens/sec, cost) — TUI already tracks this |

**Auth:** bearer token. Local mode (127.0.0.1) accepts the token from
`~/.config/openshield/desktop.token` (chmod 600, auto-generated). LAN mode
(0.0.0.0) REQUIRES the token on every request, WS included.

**Config additions (config.toml):**

```toml
[desktop]
enabled = true
bind = "127.0.0.1"        # or 0.0.0.0 for LAN/mobile
port = 9310
token_path = "~/.config/openshield/desktop.token"
pair_ttl_secs = 120        # QR pairing token lifetime
lan_mode = false           # explicit opt-in for mobile access
```

**Pairing flow:** `openshield serve --pair` (or desktop app button) calls
`POST /api/v1/pair` → returns `{host, port, token, expires}` → rendered as QR
on the desktop. Phone scans, stores creds, validates with `/api/v1/health`.

### 3.2 OpenShield Desktop — Flutter desktop app

Monorepo layout (mirrors hermes-wingman patterns):

```
openshield-desktop/
├── lib/                    # Flutter (desktop + mobile same codebase)
│   ├── main.dart
│   ├── main_shell.dart     # desktop nav rail / mobile bottom nav
│   ├── screens/            # chat, sessions, swarm, memory, skills,
│   │                       # doctor, settings, config
│   ├── services/
│   │   ├── backend_service.dart   # ChangeNotifier, connects in
│   │   │                          # background Future, NEVER blocks runApp()
│   │   ├── chat_ws.dart           # WS client, event → widget stream
│   │   └── pairing_service.dart   # QR payload parse/store (secure storage)
│   ├── widgets/            # message bubbles, tool-call cards,
│   │                       # approval cards, diff viewer, model picker
│   └── themes/             # see §5
├── rust/                   # ONLY if needed (tray, keyring); start without
└── pubspec.yaml            # com.openshield.desktop
```

Screens (v0.1):

1. **Chat** — streaming messages, tool-call cards (name, args, result,
   duration), reasoning blocks, per-session perf footer (tokens/sec).
2. **Sessions** — list/resume/archive from SQLite memory store.
3. **Approvals** — inbox of pending High/Critical security-gate requests with
   approve/reject; badge count in nav. This is the killer feature: the
   guardrail engine already assesses risk, now it can *ask a human* instead of
   only blocking.
4. **Swarm** — agent pool status, consensus doc view, stop/start.
5. **Memory** — search + recent entries.
6. **Doctor** — component health table, one-tap `--fix`.
7. **Settings/Config** — bind/port, model routing (default/weak/architect/
   editor pickers fed by `/api/v1/models`), theme picker, pair-device button
   (shows QR).

### 3.3 OpenShield Pocket — mobile app (same Flutter codebase)

Same `lib/`, platform-gated per the wingman playbook:

- **Pairing:** QR scan of the desktop's pairing payload. Wingman precedent:
  `mobile_scanner` triggers gralloc5 black screens on Android 16 (Pixel 8a) —
  v0.1 ships **manual entry + paste-payload fallback**, QR scan behind a
  feature flag until an alternative scanner package is validated. Gate the
  widget behind `Platform.isAndroid || Platform.isIOS` via a getter (never a
  library-level `final`).
- **Startup:** never block `runApp()` on connection; fast-fail
  127.0.0.1/localhost instantly; connection banner instead of LAN scanning.
- **Screens:** Chat, Sessions, Approvals (push-style badge), Settings/Config
  (last tab in bottom nav, per wingman `_mobileIndexMap` lesson).
- **Foreground-only for v0.1.** No background service, no push server. If the
  desktop is off, Pocket shows the disconnected banner and reconnects
  automatically when the LAN host answers.
- **App ID:** `com.openshield.pocket`. Test device: Pixel 8a / Android 16.

## 4. Protocol — WS Event Envelope

Extend the existing WS chat/agent streams with a typed envelope so Flutter can
dispatch without guessing:

```json
{ "type": "token",       "session": "...", "data": {"text": "..."} }
{ "type": "tool_start",  "session": "...", "data": {"name": "...", "args": {...}} }
{ "type": "tool_result", "session": "...", "data": {"name": "...", "ok": true, "ms": 42} }
{ "type": "approval_req","session": "...", "data": {"id": "...", "risk": "high",
                          "tool": "terminal", "args": {...}} }
{ "type": "approval_res","session": "...", "data": {"id": "...", "decision": "approved"} }
{ "type": "turn_done",   "session": "...", "data": {"tokens": 1234, "ms": 9100} }
{ "type": "error",       "session": "...", "data": {"message": "..."} }
```

The harness `event.rs` system already emits equivalents internally; the WS
layer just serializes them. Approvals bridge: security gate raises
`approval_req` over WS, blocks the tool call with a timeout (default 120s,
then deny), resumes on `POST /api/v1/approvals/:id`.

## 5. Theming & Branding

Flutter `AppColorScheme` pattern from Wingman (Provider +
`context.watch<ThemeManager>()`), flagship themes:

- **Blackshield** (identity theme): matte black `#101014`, steel `#8F959C`,
  blood `#C1121F`, bone `#D8D3C8`. Blood-red cross on black for the app icon.
- **Synthwave '84**: `#240037`, `#8F00FF`, `#FF7EDB`, `#FF00FF`, `#F3E70F`.

More presets port from the TUI's 24-theme system later. Icon/wordmark:
OpenShield fin + pincer motif, generated at 2560x1440-safe assets per the
branding workflow. Credit line: "Made by synth with Blackclaw."

## 6. Security Model

- All remote surfaces inherit OpenShield's L1–L4 engine: sandbox paths,
  guardrail risk assessment, PII redaction — nothing bypasses it.
- Token auth mandatory in LAN mode; token never leaves the pairing payload
  unencrypted (stored in Flutter secure storage on the phone).
- `lan_mode` defaults to **false**; binding 0.0.0.0 without it refuses to
  start (doctor warns).
- Approval timeout defaults to deny. Sudo/sensitive-path/PII checks are never
  bypassable from any client, desktop or mobile.
- v0.1 is LAN-only. WAN access (Tailscale/WireGuard) is a documented recipe,
  not a feature.

## 7. Milestones

| Milestone | Deliverable | Exit criteria |
|-----------|-------------|---------------|
| M0 — Serve hardening | `openshield serve` subcommand, token auth, `/api/v1/models`, pair endpoint | curl smoke: health, chat round-trip, pair token issues, LAN bind refuses without token |
| M1 — Desktop shell | Flutter desktop: nav, themes, sessions list, doctor panel | `flutter run -d linux`, analyze clean |
| M2 — Chat streaming | WS chat with tool cards + perf footer | live token stream vs `openshield` TUI parity |
| M3 — Approvals | WS approval_req + approvals screen, gate blocks/resumes | risky tool call pauses, approve from GUI, audit logged |
| M4 — Pocket pairing | Android APK, manual+QR pairing, chat, approvals | pairs to desktop on LAN, Pixel 8a verified |
| M5 — Extras | Swarm monitor, memory browser, model switching UI | swarm start/stop from phone |

## 8. Decisions (locked 2026-09-15)

1. Tray + autostart: systemd --user unit vs in-app tray first? (Wingman uses
   `system_tray`; desktop-only package, mobile gets the stub.) — DEFER to M5.
2. Voice input on Pocket — DEFER to v0.2.
3. **Pocket is STRICTLY REMOTE.** No offline direct-to-provider chat. Locked.
4. **Repo: separate wingman-style monorepo `openshield-desktop`.** Locked.
   Depends on the installed `openshield` binary. Flutter `lib/` for
   desktop + Pocket mobile, `docs/` for design, `server/` only if a tray or
   keyring shim is ever needed (start without).
