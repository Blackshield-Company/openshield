# 🦈 OpenShark

> *The harness that learns. The agent that decides. The tool that doesn't argue.*

OpenShark is an open-source AI coding harness that combines the best of every harness — Hermes, OpenClaw, Claude Code, Codex, OpenCode, and more — into a single, self-improving system.

It doesn't overthink. It uses model instincts, makes decisions for you, and gets better every session.

## What Makes OpenShark Different

| Feature | Other Harnesses | OpenShark |
|---------|----------------|-----------|
| **Memory** | Session-based, dies when you close | Persistent, queryable, versioned, semantic search |
| **Model Access** | Locked to one provider | Universal — any model, any provider, local or cloud |
| **Decision Making** | You choose everything | Suggests, decides, learns from results |
| **Self-Improvement** | Static prompts | Evolves prompts, routing, tools based on success data |
| **Cost Control** | Burn tokens blindly | Routes to cheapest capable model, tracks every token |
| **Agent Autonomy** | Manual tool selection | Auto-detects tool needs, plans and executes autonomously |
| **Open Source** | Proprietary | Fully open, community-driven |

## Core Philosophy

1. **Sense of Direction** — OpenShark knows what you're building and why
2. **Instinct Over Instructions** — Uses model capabilities natively, doesn't fight them
3. **Decides For You** — Picks the right model, tool, and approach based on data
4. **Learns From Itself** — Every session makes the next one better
5. **Easy On, Hard Off** — 60 seconds to start, impossible to leave

## Quick Start

```bash
cargo install openshark
openshark setup
openshark
```

## Architecture

```
┌─────────────────────────────────────────┐
│         OpenShark TUI (Ratatui)         │
│    Keyboard-driven, fast, beautiful     │
└─────────────────────────────────────────┘
                    │
    ┌───────────────┼───────────────┐
    ▼               ▼               ▼
┌────────┐    ┌──────────┐    ┌──────────┐
│ Router │    │  Memory  │    │  Tools   │
│ Engine │◄──►│  Store   │◄──►│ (git, fs,│
│        │    │(SQLite)  │    │  term)   │
└────────┘    └──────────┘    └──────────┘
    │               │               │
    ▼               ▼               ▼
┌─────────────────────────────────────────┐
│      Provider Abstraction Layer         │
│  OpenAI, Anthropic, xAI, local, etc.   │
│  LiteLLM-compatible + native opts       │
└─────────────────────────────────────────┘
    │               │               │
    ▼               ▼               ▼
┌──────────┐ ┌──────────┐ ┌──────────────┐
│  Agent   │ │  Cache   │ │ Self-Improve │
│  Loop    │ │  Store   │ │   Engine     │
└──────────┘ └──────────┘ └──────────────┘
```

## Commands

| Command | Description |
|---------|-------------|
| `openshark` | Start TUI session |
| `openshark setup` | Configure providers, models, preferences |
| `openshark stats` | View token usage, success rates, model performance |
| `openshark memory <query>` | Query persistent memory (keyword search) |
| `openshark memory <query> --semantic` | Semantic memory search |
| `openshark memory --recent` | List recent sessions |
| `openshark route` | Show current routing decisions |
| `openshark learn` | Trigger self-improvement analysis |
| `openshark agent "<task>"` | Execute task autonomously |
| `openshark test run .` | Run tests (auto-detect framework) |

## TUI Commands

Once in the TUI:

| Command | Description |
|---------|-------------|
| `help` | Show available commands |
| `tools` | List available tools |
| `history` | Show session history |
| `context` | Show memory hierarchy summary |
| `agent: <task>` | Trigger autonomous agent mode |
| `what did we do about <x>?` | Natural memory query |
| `exit` | End session |

## Tools

| Tool | Purpose | Example |
|------|---------|---------|
| `edit` | Multi-file editing | `TOOL:edit read src/main.rs` |
| `fs` | File system operations | `TOOL:fs list src/` |
| `git` | Git operations | `TOOL:git status` |
| `lsp` | LSP queries | `TOOL:lsp symbols src/main.rs` |
| `refactor` | Code refactoring | `TOOL:refactor rename_symbol src/main.rs 10 5 new_name` |
| `search` | Codebase search | `TOOL:search fn main --ext rust` |
| `grep` | Regex search | `TOOL:grep async fn src/` |
| `terminal` | Shell execution | `TOOL:terminal cargo test` |
| `test` | Test runner | `TOOL:test run .` |

## Features

### 🤖 Agentic Loop
Type `agent: fix the bug in src/main.rs` and OpenShark will:
1. Generate a plan with specific steps
2. Ask for your approval (approve/edit/reject)
3. Execute each step with verification
4. Retry failed steps (up to 3 times)
5. Escalate to recovery plan if needed

### 🧠 Semantic Memory
OpenShark remembers everything across sessions:
- **Keyword search**: `openshark memory "auth"`
- **Semantic search**: `openshark memory "auth" --semantic`
- **Natural queries**: Just ask "what did we do about auth?"
- **Context injection**: Automatically injects relevant past context into new sessions

### 🎯 Smart Routing
Automatically picks the best model for each task:
- Historical success rates (40%)
- Capability matching (35%)
- Cost efficiency (25%)
- Context length enforcement
- Budget limits

### 📊 Self-Improvement
Analyzes every session to get better:
- Model performance trends
- Tool failure pattern detection
- Prompt effectiveness ranking
- Session quality scoring
- Actionable recommendations

## Config

```toml
# ~/.config/openshark/config.toml
version = "0.1.0"
default_model = "kimi-k2.6"
auto_route = true
cost_limit_usd = 10.0

[providers.kimi]
base_url = "https://api.kimi.com/coding/v1"
api_key = "${KIMI_API_KEY}"

[[providers.kimi.models]]
name = "kimi-k2.6"
context_length = 128000
cost_per_1k_input = 0.0
cost_per_1k_output = 0.0
capabilities = ["code", "chat", "analysis"]
```

## Development

```bash
# Clone and build
git clone https://github.com/synthalorian/openshark
cd openshark
cargo build --release

# Run tests
cargo test

# Run with local model
cargo run --

# Run agent mode
cargo run -- agent "refactor the auth module"
```

## License

MIT — The future of coding belongs to everyone.
