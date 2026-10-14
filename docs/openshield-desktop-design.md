# OpenShield Desktop — Design Document

> The shield gets a face. ⚫🦞
> Working title: **OpenShield Desktop** (GUI for the OpenShield CLI harness) + **OpenShield Pocket** (mobile companion).
> Made by synth.

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
│  │ Desktop          │─────▶│ (axum REST + WS, existing    │  │
│  │ Tauri2+Svelte5   │ WS+  │  src/api/, extended)         │  │
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
                              │ Pocket (Tauri 2  │
                              │ Android)         │
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
| POST   | /api/v1/swarm/start | Start swarm (idempotent; optional `{seed, roles, max_agents}` body) |
| POST   | /api/v1/swarm/stop | Stop swarm (idempotent) |
| POST   | /api/v1/pair | Issue a short-lived pairing token + payload for QR |
| GET    | /api/v1/perf | Per-session perf (tokens/sec, cost) — TUI already tracks this |

**Auth:** bearer token, required on every request including loopback.
`openshield serve` reads `~/.config/openshield/desktop.token` (chmod 600,
auto-generated). HTTP clients send `Authorization: Bearer <token>`.
WebSocket upgrades may also pass `?token=` (browsers cannot set headers on
the handshake). LAN binds still require `lan_mode = true`.

**Config additions (config.toml):**

```toml
[desktop]
enabled = true
bind = "127.0.0.1"        # or 0.0.0.0 for LAN/mobile
port = 9310
token_path = "~/.config/openshield/desktop.token"
pair_ttl_secs = 120        # QR pairing token lifetime
lan_mode = false           # explicit opt-in for mobile access
allowed_hosts = []         # extra Host names (DNS-rebinding allowlist)
allowed_origins = []       # extra CORS origins
```

**Pairing flow:** `openshield serve --pair` (or desktop app button) calls
`POST /api/v1/pair` → returns `{host, port, token, expires}` → rendered as QR
on the desktop. Phone scans, stores creds, validates with `/api/v1/health`.

**Swarm payload (M5, implemented 2026-09-16):** `GET /api/v1/swarm` always
returns 200. With no swarm active it returns the offline state
(`running: false`, empty `agents`/`consensus`, `last_event: null`) — never
404/500. Active shape:

```json
{
  "running": true,
  "enabled": true,
  "agent_count": 4,
  "agents": [
    { "id": "architect-1", "name": "Architect 1", "role": "Architect",
      "status": "working", "model": "…", "task": "Design the system…",
      "last_activity": "…", "cycles_completed": 0, "errors_count": 0,
      "success_rate": 1.0 }
  ],
  "consensus": { "entry_count": 1, "pending": 1, "approved": 0, "rejected": 0,
                 "entries": [{ "id": "…", "author": "…", "task": "…",
                               "status": "pending", "approvals": 1,
                               "rejections": 0, "timestamp": 1758000000 }] },
  "cycles": { "completed": 1, "limit": 50 },
  "last_event": { "type": "work_completed", "agent": "architect-1", "at": "…" }
}
```

`POST /api/v1/swarm/start` / `stop` return `{success: true, state: <snapshot>,
message: "…"}`; both are idempotent (start on a running swarm and stop on an
idle one are successes, not errors). Failures are `{error: "…"}` with a 4xx/5xx
status. The endpoints drive the same `swarm::SwarmEngine` the TUI uses —
start/stop from the phone spawns real agent runs against `[swarm]` config, with
optional body overrides for the seed prompt, roles, and agent cap.
`GET /api/v1/memory/search` is a pure alias of `GET /api/v1/memory` (same
handler, query params, and response shape) — the canonical endpoint is kept
intact for existing clients.

### 3.2 OpenShield Desktop — Tauri 2 + Svelte 5 app

Stack mirrors OpenClaw Pincers (~/Projects/active/openclaw-pincers) —
Tauri 2 + SvelteKit 2 (adapter-static) + TypeScript + Rust in `src-tauri/`.
One codebase serves desktop (Linux/macOS/Windows) and Pocket (Android via
Tauri mobile, `src-tauri/gen/android`) — a pairing flow Pincers already
proved on the target device.

Monorepo layout:

```
openshield-desktop/
├── AGENTS.md               # agent brief / hard rules
├── docs/                   # design docs
├── src/                    # SvelteKit frontend (routes, components)
│   ├── lib/services/       # backend.ts (REST+WS client), pairing.ts
│   ├── lib/themes/         # CSS-variable theme system
│   └── routes/             # chat, sessions, approvals, swarm,
│                           # memory, doctor, settings
├── static/                 # assets (icons, wordmark)
├── src-tauri/              # Rust shell: window config, tray, keyring,
│   │                       # secure token storage, tauri commands
│   └── gen/android/        # Pocket Android target
└── package.json            # com.openshield.desktop
```

Frontend talks to `openshield serve` over plain HTTP fetch + native
WebSocket (not Tauri IPC) — the same code path works on desktop, Android,
and browser dev. CSP must allow `connect-src 'self' http://127.0.0.1:*
ws://127.0.0.1:* http://* ws://*` (LAN hosts for Pocket).

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

### 3.3 OpenShield Pocket — Android app (same Tauri codebase)

Same SvelteKit frontend + `src-tauri/gen/android` target, platform-gated
per the Pincers playbook:

- **Pairing:** Pincers already proves Tauri-Android pairing on the target
  Pixel 8a — QR scan or manual entry of the `/api/v1/pair` payload
  (host/port/token), validated against `/api/v1/health`. Manual entry is
  the guaranteed fallback; QR via a scanner plugin validated on-device
  before shipping.
- **Startup:** connection starts in the background and never blocks first
  render; disconnected state shows a tappable banner opening connection
  settings; no LAN subnet scanning.
- **Screens:** Chat, Sessions, Approvals (badge), Settings — bottom nav
  on mobile widths, same routes as desktop.
- **Foreground-only for v0.1.** No background service, no push server.
  Desktop unreachable = disconnected banner + auto-retry.
- **App ID:** `com.openshield.pocket`. Test device: Pixel 8a / Android 16
  (Pincers' pairing already works on this exact device).

## 4. Protocol — WS Event Envelope

### Inbound (client → server, /ws/v1/chat)

The chat frame the server consumes (the desktop GUI's adapted shape;
`session_id` is still accepted as a legacy alias for `session`):

```json
{ "type": "chat", "session": "...", "message": "..." }
```

Optional fields: `model` (override the configured default for the turn).
`session` may be omitted — the server mints a fresh session id and returns it
on every outbound envelope. Other inbound types: `{ "type": "ping" }` (→
`pong`), `{ "type": "agent", "task": "...", "yolo": false, "max_turns": 50 }`
on /ws/v1/agent.

### Outbound (server → client, typed envelope)

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

Server implementation notes (M3, 2026-09-16):

- `approval_req` / `approval_res` are broadcast to **every** connected
  `/ws/v1/chat` client; dispatch by `session` + `data.id`, not by which
  socket you sent the chat on (the approving human may be on Pocket while
  the turn runs on Desktop).
- `approval_req.data` additionally carries `reason` (the L4 gate's
  explanation) and `expires` (RFC 3339 deadline; deny-on-timeout per §6).
- The human gate engages only when **at least one** WS client is connected
  AND the assessed risk is High/Critical. With no client connected, the
  serve path behaves exactly as before the bridge existed — the gate is
  never weakened.
- REST: `GET /api/v1/approvals` lists pending
  `{id, session, risk, tool, args, reason, created_at}`;
  `POST /api/v1/approvals/:id` takes `{decision: "approve" | "reject" |
  "approve_always"}` (`approve_always` auto-approves future calls to the
  same tool for the server's lifetime).

## 5. Theming & Branding

CSS-variable theme system with a Svelte store (Pincers' 13-theme approach
is the reference), flagship themes:

- **Blackshield** (identity theme): matte black `#101014`, steel `#8F959C`,
  blood `#C1121F`, bone `#D8D3C8`. Blood-red cross on black for the app icon.
- **Synthwave '84**: `#240037`, `#8F00FF`, `#FF7EDB`, `#FF00FF`, `#F3E70F`.

More presets port from the TUI's 24-theme system later. Icon/wordmark:
OpenShield fin + pincer motif, generated at 2560x1440-safe assets per the
branding workflow. No bot or AI credit lines.

## 6. Security Model

- All remote surfaces inherit OpenShield's L1–L4 engine: sandbox paths,
  guardrail risk assessment, PII redaction — nothing bypasses it.
- Token auth mandatory on every `serve` request, including loopback; token never leaves the pairing payload
  unencrypted (stored via Tauri stronghold/keyring on desktop and Android
  keystore-backed storage on Pocket).
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

1. Tray + autostart: systemd --user unit vs in-app tray first? — DEFER to M5.
2. Voice input on Pocket — DEFER to v0.2.
3. **Pocket is STRICTLY REMOTE.** No offline direct-to-provider chat. Locked.
4. **Repo: separate wingman-style monorepo `openshield-desktop`.** Locked.
   Depends on the installed `openshield` binary.
5. **Stack: Tauri 2 + SvelteKit 2 (adapter-static) + TypeScript + Rust
   `src-tauri/`.** Locked — supersedes the original Flutter choice. Rationale:
   OpenClaw Pincers (~/Projects/active/openclaw-pincers, v0.3.3) ships this
   exact stack cross-platform INCLUDING Android with working device pairing
   on the target Pixel 8a; same patterns transfer; Rust shell sits next to
   openshield serve. The Flutter M1 scaffold remains in git history only.
