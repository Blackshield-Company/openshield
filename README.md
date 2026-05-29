# 🦈 OpenShark

> *The harness that learns. The agent that decides. The tool that doesn't argue.*

OpenShark is an open-source AI coding harness that combines the best of every harness — Hermes, OpenClaw, Claude Code, Codex, OpenCode, and more — into a single, self-improving system.

It doesn't overthink. It uses model instincts, makes decisions for you, and gets better every session.

## What Makes OpenShark Different

| Feature | Other Harnesses | OpenShark |
|---------|----------------|-----------|
| **Memory** | Session-based, dies when you close | Persistent, queryable, versioned |
| **Model Access** | Locked to one provider | Universal — any model, any provider, local or cloud |
| **Decision Making** | You choose everything | Suggests, decides, learns from results |
| **Self-Improvement** | Static prompts | Evolves prompts, routing, tools based on success data |
| **Cost Control** | Burn tokens blindly | Routes to cheapest capable model, tracks every token |
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
    │
    ▼
┌─────────────────────────────────────────┐
│      Provider Abstraction Layer         │
│  OpenAI, Anthropic, xAI, local, etc.   │
│  LiteLLM-compatible + native opts       │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│      Self-Improvement Engine            │
│  Prompt evolution, routing optimization,│
│  tool refinement, success tracking      │
└─────────────────────────────────────────┘
```

## Commands

| Command | Description |
|---------|-------------|
| `openshark` | Start TUI session |
| `openshark setup` | Configure providers, models, preferences |
| `openshark stats` | View token usage, success rates, model performance |
| `openshark memory` | Query persistent memory |
| `openshark route` | Show current routing decisions |
| `openshark learn` | Trigger self-improvement analysis |

## License

MIT — The future of coding belongs to everyone.
