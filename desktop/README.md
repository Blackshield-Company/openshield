# OpenShark Desktop

> 🦈 *The harness, unchained from the terminal.*

Standalone GUI for [OpenShark](../openshark) — Svelte 5 + Tauri v2 + Rust, wrapped in a neon 1984 synthwave shell.

![Retro soul, modern precision](https://img.shields.io/badge/aesthetic-synthwave-neonpink)

## What It Does

Wraps the `openshark` CLI in a native desktop window:

- **Dashboard** — binary status, API server control, session stats
- **Chat** — streaming conversation with the shark (token-level via WebSocket)
- **Agent** — autonomous task runner with structured thinking/tool events
- **Memory** — search the persistent memory vault (semantic / recent / keyword)
- **Models** — browse configured providers
- **Tools** — the full arsenal
- **Doctor** — diagnostics with one-click fix
- **Config** — edit `config.toml` in place

## Server Mode

On launch the app boots `openshark serve` on `127.0.0.1:1984` (adopting an
already-running instance if found). With the server up:

- **Chat** streams real token deltas over `/ws/v1/chat` (with `<think>`-tag
  filtering and a per-message model override)
- **Agent** renders structured `thinking` / `tool_call` / `tool_result` /
  `complete` events from `/ws/v1/agent` instead of raw text

Without it, both views fall back to spawning the CLI directly. The sidebar
shows an `⚡ api :1984` badge when server mode is live. Servers spawned by the
app are killed on exit; adopted ones are left alone.

## Requirements

- `openshark` binary on `PATH` (v1.1.0+)
- Node.js 18+
- Rust 1.91+ with Tauri v2 system deps (`webkit2gtk-4.1` on Linux)

## Themes

Twelve shark-grade themes, switchable from the 🎨 picker in the sidebar (persists across launches):

| Theme | Vibe |
|-------|------|
| 🦈 Neon Frenzy | Default — hot pink on abyssal black |
| 🌆 Synthwave '84 | The classic. Deep purple grid, electric violet sunset |
| 🤍 Great White | Clean light mode, apex predator in daylight |
| 💨 Mako | Chrome blue velocity |
| 🔨 Hammerhead | Industrial amber on gunmetal |
| 🐯 Tiger Shark | Orange stripes on black water |
| 🦷 Megalodon | Teal bioluminescence in crushing dark |
| 🩸 Blood in the Water | Crimson tide |
| 🪸 Reef | Tropical coral shallows |
| 📺 Shark Week | Documentary navy, red title card |
| 🏖️ Nurse Shark | Warm sand, zero urgency |
| 🎬 Amity '75 | JAWS-poster midnight |

## Typography

UI is set in **[3270](https://github.com/rbanffy/3270font)** — the IBM 3270 mainframe terminal face (bundled locally under `public/fonts/`, BSD-3-Clause, see `LICENSE-3270.txt`).

## Run

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run tauri build
```

AppImage bundling may fail on some systems (linuxdeploy) — the binary, .deb,
and .rpm are still produced under `src-tauri/target/release/bundle/`.

## Install (Linux launcher)

```bash
scripts/install-desktop.sh
```

Installs the app launcher + hicolor icons, pointing at the release binary.

## Release

Push a `v*` tag — GitHub Actions builds Linux/macOS/Windows bundles and
attaches them to a GitHub Release (see `.github/workflows/release.yml`).

---

*Fast. Precise. Hungry.* — This is the wave.
