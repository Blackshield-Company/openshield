# OpenShark Status — Session Handoff

## What's Built (v0.2.0)

| Feature | Status | Details |
|---------|--------|---------|
| CLI scaffold | ✅ | `setup`, `stats`, `memory`, `route`, `learn` commands |
| Provider abstraction | ✅ | OpenAI-compatible API, local llama-swap support |
| Chat with models | ✅ | **Streaming** chat, system prompts, context window |
| SQLite memory | ✅ | Sessions, messages, tool calls persisted |
| Memory search CLI | ✅ | `openshark memory <query>` + `--recent [n]` |
| Tool execution | ✅ | `fs`, `terminal`, `search`, `grep`, `git`, `edit` |
| Config management | ✅ | TOML-based, provider registry with model costs |
| Session tracking | ✅ | UUID sessions, message history, tool call logging |

## Architecture

```
src/
├── main.rs              # CLI entry (clap, async tokio)
├── config/
│   ├── mod.rs           # Config struct, load/save, defaults
│   └── setup.rs         # `openshark setup` wizard
├── memory/
│   ├── mod.rs           # Public exports
│   └── store.rs         # SQLite: sessions, messages, tool_calls
├── providers/
│   └── mod.rs           # Provider struct, chat(), chat_stream(), list_models()
├── router/
│   └── mod.rs           # Routing decisions (stub with fake data)
├── self_improve/
│   └── mod.rs           # Analysis engine (stub with fake data)
├── tools/
│   ├── mod.rs           # Tool trait, registry, find_tool()
│   ├── edit.rs          # Multi-file editing: read, write, replace, patch
│   ├── fs.rs            # File system: read, write, list
│   ├── git.rs           # Git: status, diff, log, branch, checkout, commit, add
│   ├── search.rs        # Codebase search: ripgrep + regex fallback
│   └── terminal.rs      # Shell command execution
└── tui/
    └── mod.rs           # Interactive session loop with streaming
```

## Tools Reference

| Tool | Commands | Example |
|------|----------|---------|
| `edit` | read, write, replace, patch | `TOOL:edit read src/main.rs` |
| `fs` | read, write, list | `TOOL:fs read README.md` |
| `git` | status, diff, log, branch, checkout, commit, add | `TOOL:git status` |
| `search` | ripgrep search | `TOOL:search fn main --ext rust` |
| `grep` | regex fallback | `TOOL:grep async fn src/` |
| `terminal` | shell execution | `TOOL:terminal cargo test` |

## Next Session Targets

### Priority 1: Coding Depth (continued)
- [x] Multi-file editing with diff application
- [x] Codebase search (ripgrep integration)
- [ ] LSP integration for symbol understanding
- [x] Git integration: status, diff, commit, branch
- [ ] Test runner integration

### Priority 2: Real Memory (Hermes killer)
- [ ] Semantic search with vector embeddings
- [x] Cross-session context injection (basic: search + recent)
- [ ] Memory hierarchy: session → project → global
- [ ] "What did we do about X?" query

### Priority 3: Agent Autonomy (OpenCode/OMO killer)
- [ ] Auto-tool detection from model output
- [ ] Agentic loop: plan → execute → verify → iterate
- [ ] Parallel tool execution
- [ ] Error recovery and retry logic

### Priority 4: Speed & Responsiveness
- [x] Streaming responses
- [ ] Async tool execution
- [ ] Connection pooling
- [ ] Response caching

## Key Files for Next Session

| File | Purpose |
|------|---------|
| `src/tui/mod.rs` | Main session loop — add auto-tool detection here |
| `src/memory/store.rs` | SQLite layer — add vector search here |
| `src/tools/mod.rs` | Tool registry — add new tools here |
| `src/providers/mod.rs` | API layer — streaming done, add connection pool |
| `src/router/mod.rs` | Model selection — add real routing logic here |

## Running

```bash
cd /home/synth/projects/openshark
cargo run --              # Start TUI with streaming
cargo run -- setup       # Reconfigure
cargo run -- route       # See routing decisions
cargo run -- learn       # See self-improvement analysis
cargo run -- memory "auth"        # Search memory
cargo run -- memory --recent      # List recent sessions
cargo run -- memory --recent --limit 5  # List last 5 sessions
```

## Config Location

- Config: `~/.config/openshark/config.toml`
- Memory: `~/.local/share/openshark/memory.db`

## Test It

```bash
# Chat with local model (streaming)
openshark
> hello

# Use tools directly
> TOOL:search pub struct src/
> TOOL:git status
> TOOL:edit read src/main.rs
> TOOL:terminal cargo test

# Check memory persisted
openshark memory "hello"
openshark memory --recent 5
```

## The Vision Reminder

> One harness. Universal models. Real memory. Agent autonomy. Open source.
>
> Better than Claw Code at coding. Better than Hermes at memory.
> Better than OpenCode/OMO at agent control. Better than OpenClaw at tool execution.
>
> It knows its sense of direction. It decides for you. It learns from itself.
> Easy to board, hard to get off.

---

*Session ended. Next session picks up at Priority 1: LSP integration + test runner.*
