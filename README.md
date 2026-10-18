<p align="center">
  <img src=".github/preview.png" alt="OpenShield: AI coding agent in your terminal" width="100%">
</p>

<h1 align="center">OpenShield</h1>

<p align="center">AI coding agent in your terminal. Rust. Open source. Any provider.</p>

<p align="center">
  <a href="https://github.com/Blackshield-Company/openshield/releases"><img alt="Release" src="https://img.shields.io/github/v/release/Blackshield-Company/openshield?sort=semver&display_name=tag&label=release&color=C1121F&labelColor=17171D&cacheSeconds=3600"></a>
  <a href="https://github.com/Blackshield-Company/openshield/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/Blackshield-Company/openshield/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-9AA0A8?labelColor=17171D"></a>
  <a href="https://rust-lang.org"><img alt="Rust 1.91+" src="https://img.shields.io/badge/rust-1.91%2B-D4A72C?labelColor=17171D"></a>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#first-run">First run</a> ·
  <a href="#local-models-ollama-llamacpp">Local models</a> ·
  <a href="#commands">Commands</a> ·
  <a href="docs/brand/README.md">Brand</a>
</p>

## What It Does

- **Persistent memory** — SQLite-backed sessions with keyword and semantic search. Close the terminal, come back next week, it still knows what you were doing.
- **Any model provider** — Works with any OpenAI-compatible API. OpenRouter, llama-swap, xAI, local servers. Not locked to anyone.
- **Autonomous agent mode** — Plans, executes, verifies, retries. You approve the plan, it does the work.
- **Black Steel TUI** — Void, steel, bone and blood palette, a flat shield with a plain Greek cross, and a launch screen you can start typing into.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/Blackshield-Company/openshield/main/install.sh | bash
```

Downloads the prebuilt binary for your platform (Linux x86_64/aarch64, macOS x86_64/arm64) from [GitHub Releases](https://github.com/Blackshield-Company/openshield/releases), verifies its SHA-256 checksum, and installs it to `~/.local/bin/openshield`. If no prebuilt binary matches, it falls back to `cargo install --git`.

Options: `OPENSHIELD_VERSION=v1.1.0` pins a release, `BIN_DIR=/path` changes the install directory, `OPENSHIELD_FROM_SOURCE=1` forces a source build, `DRY_RUN=1` prints the plan without installing.

Windows: download `openshield-x86_64-pc-windows-msvc.zip` from the releases page. From source anywhere: `cargo install --git https://github.com/Blackshield-Company/openshield --locked`.

## First Run

```bash
openshield setup    # Configure providers and models
openshield          # Start TUI session
```

For cloud models, set one API key environment variable before starting: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `KIMI_API_KEY`, or `XAI_API_KEY`. To run fully local with no API key, see [Local models (Ollama, llama.cpp)](#local-models-ollama-llamacpp).

## Local models (Ollama, llama.cpp)

OpenShield talks to any OpenAI-compatible server, so it can run fully local with no cloud API key. Ollama, llama.cpp `llama-server` and llama-swap all work.

**Context size matters.** The system prompt alone is about 5k tokens, and Ollama defaults to a 4096-token context. Give the model at least 16k:

```bash
printf 'FROM qwen2.5:7b\nPARAMETER num_ctx 16384\n' > Modelfile   # or FROM /path/to/model.gguf
ollama create qwen-local -f Modelfile
```

If the context is too small, OpenShield reports `Context window exceeded` and exits non-zero.

In `~/.config/openshield/config.toml`, keep the generated non-provider sections, replace the `[providers.*]` tables, and set `default_model`:

```toml
default_model = "qwen-local"

[providers.ollama]
base_url = "http://127.0.0.1:11434/v1"
api_key = "ollama"            # any non-empty dummy value; no env var needed

[[providers.ollama.models]]
name = "qwen-local"
context_length = 32768
cost_per_1k_input = 0.0
cost_per_1k_output = 0.0
capabilities = ["code", "chat"]
```

Then:

```bash
openshield models                   # qwen-local [default]
openshield chat "What is 2+2?"
openshield agent "summarize src/"   # uses default_model; override with --model
openshield headless -m qwen-local "..."
```

For llama.cpp `llama-server` or llama-swap, use the same layout with `base_url = "http://127.0.0.1:8080/v1"`. The model names must match what the server serves. The built-in `local` provider written by `openshield setup` uses example model names (`local-35b-128k`, ...), so edit them to match your server. Very small models (under ~3B parameters) work for chat but often choose bad tools in agent mode.

## Commands

| Command | Description |
|---------|-------------|
| `openshield` | Start TUI session |
| `openshield setup` | Configure providers, models, preferences |
| `openshield agent "<task>"` | Execute task autonomously |
| `openshield chat "<message>"` | One-shot chat |
| `openshield memory <query>` | Search persistent memory |
| `openshield memory <query> --semantic` | Semantic memory search |
| `openshield stats` | Token usage and model performance |
| `openshield models` | List available models |
| `openshield config` | Show configuration |
| `openshield serve` | Desktop/Pocket API. Bearer token required on every request, including loopback (`~/.config/openshield/desktop.token`; WebSocket may use `?token=`) |

## TUI Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+A` | Toggle autonomous mode |
| `Ctrl+T` | Cycle themes |
| `/multi` | Toggle multi-model mode |
| `/compare` | Show multi-model comparison |
| `↑/↓` | Navigate history |

Themes: `blackshield` (default), `steel_blue`, `high_contrast`.

## Tools

9 built-in tools: `edit`, `fs`, `git`, `lsp`, `refactor`, `search`, `grep`, `terminal`, `test`. Plus MCP server tools via native client.

## Config

Config lives at `~/.config/openshield/config.toml`. Run `openshield setup` for interactive generation.

<details>
<summary>Example config</summary>

```toml
version = "1.1.0"
default_model = "gpt-4o"
auto_route = true
cost_limit_usd = 10.0
theme = "blackshield"
# Mascot sprite on the splash screen. Default "none" shows the Black Steel
# shield; "blackshield-mercenary" brings back the knight.
# Custom sprites: ~/.config/openshield/sprites/*.ans|*.txt
sprite = "none"
# Reduced motion: the knight shows a still frame instead of animating.
# NO_MOTION=1 or REDUCE_MOTION=1 does the same; /motion on|off toggles it.
reduced_motion = false

[providers.openai]
base_url = "https://api.openai.com/v1"
api_key = "${OPENAI_API_KEY}"

[[providers.openai.models]]
name = "gpt-4o"
context_length = 128000
cost_per_1k_input = 0.005
cost_per_1k_output = 0.015
capabilities = ["code", "chat", "analysis"]
```

</details>

## Development

```bash
git clone https://github.com/Blackshield-Company/openshield
cd openshield
cargo build --release
cargo test
```

The release profile uses fat LTO, and its link step can need more than 15 GB of RAM. On smaller machines, turn LTO off for a local build (the binary is slightly larger and slower; the shipped profile is unchanged):

```bash
CARGO_PROFILE_RELEASE_LTO=false cargo build --release -j4
```

## More

- [CHANGELOG.md](CHANGELOG.md) — Version history
- [docs/planning/ROADMAP.md](docs/planning/ROADMAP.md) — Future plans
- [docs/planning/STATUS.md](docs/planning/STATUS.md) — Current development status
- [docs/planning/](docs/planning/) — Audits, plans and release notes
- [docs/brand/](docs/brand/README.md) — Logo, wordmark, colors and type

## License

Apache License 2.0. See [LICENSE](LICENSE).

Security issues: see [SECURITY.md](SECURITY.md).

---

## ☕ Support the Developer

If this project saved you time, solved a problem, or kept your terminal a little more steel-forged, you can fuel the next one:

[![Buy Me A Coffee](https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png)](https://buymeacoffee.com/synthalorian)

## Demo: web terminal + mock provider (no API keys)

```bash
cargo run --release -- demo                 # open http://127.0.0.1:7681
cargo run --release -- demo --bind 0.0.0.0 --port 8080
cargo run --release -- mock-provider        # mock OpenAI-compatible API only, http://127.0.0.1:7682/v1
```

`openshield demo` starts a built-in mock model (canned replies, no network), writes an
isolated config whose only provider is that mock, and serves a browser terminal
(xterm.js over WebSocket) running the full TUI. Your own `~/.config/openshield` is not used.

| Env var | Default | Purpose |
|---|---|---|
| `OPENSHIELD_DEMO_PORT` | `7681` | Web terminal port (`--port` overrides) |
| `OPENSHIELD_DEMO_BIND` | `127.0.0.1` | Bind address (`--bind` overrides) |
| `OPENSHIELD_DEMO_DIR` | `$TMPDIR/openshield-demo` | Isolated config/data/workspace dir |
| `OPENSHIELD_MOCK_PORT` | `7682` | Port for `mock-provider` |

The web terminal has no authentication and gives a shell-capable agent session, so only
expose it beyond loopback on a sandboxed host.
