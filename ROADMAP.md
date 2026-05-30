# OpenShark Roadmap

## Phase 1: Core Engine (Weeks 1-2)
**Goal:** Chat with any model, persistent memory, basic tools.
**Status: ✅ COMPLETE** — v0.1.0 shipped with chat, SQLite memory, fs/terminal tools

## Phase 2: Coding Depth (Weeks 3-5)
**Goal:** Match Claw Code / Claude Code for coding tasks.
**Status: ✅ COMPLETE** — v0.2.0 with multi-file editing, codebase search, LSP integration, git operations, test runner, refactor engine

| Week | Milestone | Deliverable |
|------|-----------|-------------|
| 3 | Multi-file editing | Diff-based file modifications |
| 3 | Codebase search | ripgrep integration, symbol search |
| 4 | LSP integration | Go-to-definition, type info, diagnostics |
| 4 | Git integration | status, diff, commit, branch, checkout |
| 5 | Test runner | Auto-detect test framework, run tests |
| 5 | Refactor engine | Extract function, rename symbol, etc. |

**Success Criteria:** Can build a feature end-to-end without leaving OpenShark. ✅

## Phase 3: Real Memory (Weeks 6-7)
**Goal:** Surpass Hermes memory with semantic search and cross-session context.
**Status: ✅ COMPLETE** — v0.3.0 with vector embeddings, memory hierarchy, context injection, natural queries

| Week | Milestone | Deliverable |
|------|-----------|-------------|
| 6 | Vector embeddings | Hash-based semantic vectors (1000-dim) |
| 6 | Memory hierarchy | session → project → global context layers |
| 7 | Context injection | Auto-inject relevant past sessions |
| 7 | Natural queries | "What did we do about auth?" → instant answer |

**Success Criteria:** 3-week-old context recalled in < 2 seconds. ✅

## Phase 4: Agent Autonomy (Weeks 8-9)
**Goal:** Exceed OpenCode/OMO agent control.
**Status: ✅ COMPLETE** — v0.4.0 with auto-tool detection, agentic loop, parallel execution, error recovery

| Week | Milestone | Deliverable |
|------|-----------|-------------|
| 8 | Auto-tool detection | Model suggests tools, user approves |
| 8 | Agentic loop | plan → execute → verify → iterate |
| 9 | Parallel execution | Multiple tools concurrently |
| 9 | Error recovery | Retry, fallback, escalation logic |

**Success Criteria:** "Fix the bug" → finds, fixes, tests, commits without guidance. ✅ (with user plan approval)

## Phase 5: Speed & Polish (Weeks 10-12)
**Goal:** Faster and more responsive than any harness.
**Status: 🔄 IN PROGRESS** — Streaming ✅, async tools ✅, connection pool ✅, response cache ✅, TUI polish ❌

| Week | Milestone | Deliverable | Status |
|------|-----------|-------------|--------|
| 10 | Streaming | Real-time token streaming | ✅ |
| 10 | Async tools | Non-blocking tool execution | ✅ |
| 11 | Connection pool | Reuse connections, reduce latency | ✅ |
| 11 | Response cache | Cache common responses | ✅ |
| 12 | TUI polish | Ratatui interface, themes, keybindings | 🔄 |

**Success Criteria:** First token in < 500ms, tool results in < 1s.

## Phase 6: Distribution & Community (Week 13+)
**Goal:** Make OpenShark accessible to everyone.
**Status: 📋 PLANNED**

| Milestone | Deliverable |
|-----------|-------------|
| Stats command | Real token usage, cost tracking, performance metrics |
| Multi-model chat | Compare responses side-by-side |
| Custom tools | User-defined tools via config |
| Session branching | Fork sessions to explore alternatives |
| Cargo publish | crates.io publication |
| Installation scripts | One-liner curl install |
| Package managers | Homebrew, AUR, etc. |

## Phase 7: Agent Identity (v0.5.0)
**Goal:** Per-user customizable agent personality, name, and branding.
**Status: ✅ COMPLETE**

| Feature | Status | Description |
|---------|--------|-------------|
| Config-based identity | ✅ | Agent name, role, origin, tagline in config.toml |
| Personality system | ✅ | Tone, style, greeting, farewell customizable |
| Emoji/branding | ✅ | Per-user emoji, catchphrases, behavioral rules |
| TUI integration | ✅ | Sidebar shows user's agent name, welcome uses identity |
| Setup wizard | ✅ | Interactive agent identity configuration |

**How to customize your agent:**

1. **Via setup wizard:** `openshark setup` → configure agent identity interactively
2. **Via config edit:** Edit `~/.config/openshark/config.toml`:
```toml
[agent]
name = "myagent"
display_name = "MyAgent"
role = "coding assistant"
origin = "Created in the neon grid"
purpose = "To ship code fast"
tagline = "Let's build the future."
tone = "Professional but friendly"
style = "Concise and thorough"
greeting = "Hey! Ready to code?"
farewell = "See you next session!"
emoji = "🤖"
catchphrases = ["Let's do this!", "Ship it!"]
behavioral_rules = [
    "Always verify before claiming success",
    "Show the code, don't just describe it",
]
```

3. **Environment override:** `SOUL_NAME=blank` for neutral assistant

## Phase 8: Infrastructure & Platform (v0.6.0)
**Goal:** OpenShark becomes a full agent platform — not just a coding harness. Gateway, MCP, skills, self-improvement, multi-platform messaging.
**Status: 📋 PLANNED**

### 8.1 Setup System with Config Transfer

OpenShark's setup wizard runs standalone and can optionally import from other agent configs.

```bash
openshark setup                              # Interactive setup wizard
openshark setup --migrate-from hermes        # Import Hermes config
openshark setup --migrate-from openclaw      # Import OpenClaw config
openshark setup --migrate-from hermes --dry-run   # Preview only
```

**Setup Flow:**
```
1. DETECT  → 2. INSTALL DEPS (if needed)  → 3. AUTO-CONFIGURE  → 4. TEST  → 5. DONE
```

**Step 1 — Detect:**
- Check Rust toolchain (cargo, rustc)
- Check for existing OpenShark config at `~/.config/openshark/`
- Detect Hermes installation at `~/.hermes` (offer config transfer)
- Detect OpenClaw installation at `~/.openclaw` (offer config transfer)

**Step 2 — Install Dependencies:**
- Rust toolchain (if missing, prompt for rustup install)
- Build dependencies (openssl, pkg-config)

**Step 3 — Auto-Configure:**
- Write `~/.config/openshark/config.toml` with defaults
- Create `~/.local/share/openshark/` for memory database
- Create `~/.cache/openshark/` for response cache
- Generate shell completions (bash, zsh, fish)

**Step 4 — Test:**
- Verify binary builds successfully
- Test provider connectivity (if API keys configured)
- Test memory database initialization

**Step 5 — Done:**
- Print summary of configured settings
- Show quick-start commands
- Offer to launch TUI

**Config Transfer from Hermes → OpenShark:**

| Hermes Source | OpenShark Destination | Content |
|---------------|----------------------|---------|
| `~/.hermes/SOUL.md` | `~/.config/openshark/SOUL.md` | User persona / agent identity |
| `~/.hermes/memory/` | `~/.local/share/openshark/memory/` | Hermes memory entries |
| `~/.hermes/skills/` | `~/.config/openshark/skills/` | Skills (filtered for coding/dev) |
| `~/.hermes/config.yaml` | `~/.config/openshark/config.toml` | Provider configs (mapped to OpenShark format) |

**Config Transfer from OpenClaw → OpenShark:**

| OpenClaw Source | OpenShark Destination | Content |
|-----------------|----------------------|---------|
| `~/.openclaw/SOUL.md` | `~/.config/openshark/SOUL.md` | User persona / agent identity |
| `~/.openclaw/MEMORY.md` | `~/.local/share/openshark/memory/` | Long-term agent knowledge |
| `~/.openclaw/USER.md` | `~/.config/openshark/user_profile.md` | User profile |
| `~/.openclaw/workspace/tts/` | `~/.config/openshark/tts/` | TTS voice assets |
| `~/.openclaw/skills/` | `~/.config/openshark/skills/` | User skills (filtered for coding/dev) |
| `~/.openclaw/.env` | `~/.config/openshark/*.env` | API keys (Hermes-compatible providers only) |

**Migration Paths (No Circular Deps):**
- **OpenClaw → Hermes:** `hermes claw migrate` (Hermes maintains this)
- **OpenClaw → OpenShark:** `openshark setup --migrate-from openclaw`
- **Hermes → OpenShark:** `openshark setup --migrate-from hermes`

Each tool only reads from source, never writes to another tool's config.

### 8.2 Doctor — Auto-Repair System

`openshark doctor` is not just a diagnostic — it's an auto-repair function that detects and fixes broken components.

```bash
openshark doctor              # Full diagnostic + auto-fix
openshark doctor --check      # Diagnostic only, no fixes
openshark doctor --fix        # Apply all fixes without prompting
openshark doctor --component gateway   # Check/fix only gateway
openshark doctor --component mcp      # Check/fix only MCP
openshark doctor --component skills   # Check/fix only skills
openshark doctor --component memory   # Check/fix only memory
openshark doctor --component providers # Check/fix only providers
```

**What doctor checks and fixes:**

| Component | Checks | Fixes |
|-----------|--------|-------|
| **Config** | Config file exists, valid TOML, required fields present | Rewrite missing fields with defaults, backup old config |
| **Providers** | API keys valid, endpoints reachable, models listable | Regenerate env files, prompt for new keys, test connectivity |
| **Memory DB** | SQLite file exists, schema current, no corruption | Recreate from schema, rebuild embeddings index |
| **Cache** | Cache file exists, not oversized, valid JSON | Clear stale entries, rebuild if corrupted |
| **Gateway** | Gateway process running, platforms connected, tokens valid | Restart gateway, refresh tokens, re-register webhooks |
| **MCP** | MCP servers configured, connections active, tools discoverable | Re-register servers, restart connections, clear stale configs |
| **Skills** | Skills directory exists, YAML valid, no duplicates | Re-index skills, remove duplicates, fetch updates |
| **Self-Improve** | Improvement DB exists, metrics collecting, trends analyzable | Rebuild metrics DB, re-seed baseline data |
| **Discord** | Bot token valid, gateway connected, permissions correct | Regenerate token if expired, re-request permissions |
| **Telegram** | Bot token valid, webhook set, polling active | Reset webhook, re-register bot |
| **Rust Build** | Cargo lock valid, deps resolve, binary builds | `cargo update`, clear fingerprints, rebuild |

**Doctor Architecture:**
```rust
pub trait DoctorCheck: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self) -> DoctorResult;
    fn fix(&self, issue: &Issue) -> FixResult;
    fn can_fix(&self, issue: &Issue) -> bool;
}

pub struct Doctor {
    checks: Vec<Box<dyn DoctorCheck>>,
    auto_fix: bool,
}

impl Doctor {
    pub fn run(&self) -> DoctorReport {
        // Run all checks, collect issues
        // For each fixable issue: prompt user or auto-fix
        // Return report with before/after state
    }
}
```

### 8.3 OpenShark Gateway (Native — No Hermes Dependency)

OpenShark runs its own gateway for multi-platform messaging. No Hermes required.

```bash
openshark gateway start       # Start gateway daemon
openshark gateway stop        # Stop gateway daemon
openshark gateway status      # Show platform connections
openshark gateway restart     # Restart gateway
openshark gateway logs        # Tail gateway logs
```

**Supported Platforms:**
- Discord (bot + slash commands)
- Telegram (bot + inline queries)
- Slack (app + socket mode)
- Matrix (homeserver integration)
- Webhooks (HTTP callbacks)
- API Server (REST API for external tools)

**Gateway Architecture:**
```
┌─────────────────────────────────────────┐
│           OpenShark Gateway             │
│  ┌─────────┐ ┌─────────┐ ┌──────────┐ │
│  │ Discord │ │Telegram │ │  Slack   │ │
│  │ Adapter │ │ Adapter │ │ Adapter  │ │
│  └────┬────┘ └────┬────┘ └────┬─────┘ │
│       │           │           │        │
│  ┌────┴───────────┴───────────┴─────┐  │
│  │      Message Router + Queue      │  │
│  └────────────────┬─────────────────┘  │
│                   │                     │
│  ┌────────────────┴─────────────────┐  │
│  │       OpenShark Agent Core       │  │
│  │  (tools, memory, model routing)  │  │
│  └──────────────────────────────────┘  │
└─────────────────────────────────────────┘
```

**Config:**
```toml
[gateway]
enabled = true
bind_address = "127.0.0.1:7654"
max_connections = 100

[gateway.discord]
enabled = true
token = "${DISCORD_BOT_TOKEN}"
command_prefix = "!"
slash_commands = true

[gateway.telegram]
enabled = false
token = "${TELEGRAM_BOT_TOKEN}"
webhook_url = "https://your-domain.com/webhook"

[gateway.slack]
enabled = false
app_token = "${SLACK_APP_TOKEN}"
bot_token = "${SLACK_BOT_TOKEN}"
socket_mode = true
```

### 8.4 MCP Server Integration

OpenShark acts as an MCP client — discovers and calls tools from any MCP server.

```bash
openshark mcp list            # List configured MCP servers
openshark mcp add <name>      # Add MCP server
openshark mcp remove <name>   # Remove MCP server
openshark mcp test <name>     # Test server connection
openshark mcp tools <name>    # List tools from server
```

**Config:**
```toml
[[mcp.servers]]
name = "filesystem"
type = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/synth"]

[[mcp.servers]]
name = "github"
type = "sse"
url = "http://localhost:3000/sse"
```

**Integration:** MCP tools appear alongside native tools in the tool registry. The agent can call them transparently.

### 8.5 Skills System

OpenShark has its own skills system — independent from Hermes.

```bash
openshark skills list         # List installed skills
openshark skills search       # Search skills hub
openshark skills install      # Install skill from hub
openshark skills update       # Update all skills
openshark skills remove       # Remove skill
```

**Skill Format:**
```yaml
---
name: my-skill
description: What this skill does
triggers:
  - keyword1
  - keyword2
tags:
  - rust
  - web
---

# Skill content (markdown)
```

**Skill Directories:**
- Built-in: `~/.config/openshark/skills/builtin/`
- User: `~/.config/openshark/skills/user/`
- Hub: `~/.config/openshark/skills/hub/`

### 8.6 Self-Improvement Engine

OpenShark analyzes its own performance and suggests improvements.

```bash
openshark learn               # Run self-improvement analysis
openshark learn --report      # Generate improvement report
openshark learn --apply       # Apply recommended changes
```

**What it tracks:**
- Tool success/failure rates per tool
- Model performance per task type
- Response latency trends
- Memory retrieval accuracy
- User correction patterns

**What it improves:**
- Routing weights (which model for which task)
- Tool selection confidence thresholds
- Memory embedding quality
- Context compression strategies

### 8.7 Files to Create

```
src/config/setup.rs              # Enhanced setup wizard with migration
src/config/migrate_hermes.rs     # Hermes → OpenShark migration
src/config/migrate_openclaw.rs   # OpenClaw → OpenShark migration
src/doctor/mod.rs                # Doctor auto-repair system
src/doctor/checks.rs             # Individual health checks
src/doctor/fixes.rs              # Auto-fix implementations
src/gateway/mod.rs               # Gateway daemon
src/gateway/discord.rs           # Discord adapter
src/gateway/telegram.rs          # Telegram adapter
src/gateway/slack.rs             # Slack adapter
src/gateway/matrix.rs            # Matrix adapter
src/gateway/router.rs            # Message routing
src/mcp/mod.rs                   # MCP client
src/mcp/discovery.rs             # Server discovery
src/mcp/tools.rs                 # Tool registry bridge
src/skills/mod.rs                # Skills system
src/skills/hub.rs                # Skills hub client
src/skills/loader.rs             # Skill loader/parser
src/self_improve/mod.rs          # Self-improvement engine
src/self_improve/metrics.rs      # Metrics collection
src/self_improve/analysis.rs     # Trend analysis
scripts/setup.sh                 # One-liner curl install
scripts/doctor.sh                # Standalone doctor script
```

## Success Metrics

| Phase | Metric | Status |
|-------|--------|--------|
| 1 | Can chat with any model, memory persists | ✅ |
| 2 | Routing beats random selection by 30% | ✅ |
| 3 | Feature build without leaving TUI | ✅ |
| 4 | Self-improvement measurably better | ✅ |
| 5 | First token < 500ms, tools < 1s | 🔄 |
| 6 | 100+ GitHub stars, 10+ contributors | 📋 |
| 7 | Per-user agent identity working | ✅ |
| 8 | Gateway running, MCP connected, skills loaded, doctor auto-fixing | 📋 |
