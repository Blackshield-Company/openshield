# OpenShark v1.2.x Roadmap

## Vision

OpenShark becomes the **synthesis engine** — a standalone powerhouse that optionally integrates with every other harness (Hermes, OpenClaw, OpenCode, Claw Code, Claude Code). Strong alone, stronger connected. No circular deps, no required external tools.

---

## Tier 1: Git Agent (Code Agent Parity) ✅ COMPLETE

**Goal:** Match Claw Code / Claude Code / OpenCode for the core coding loop.

| Feature | Command | Description |
|---------|---------|-------------|
| Smart commit | `/commit [msg]` | Stage all changes, generate commit message with LLM, show diff for approval |
| PR creation | `/pr [title]` | Branch, commit, push, open PR with generated description |
| Test runner | `/test` | Auto-detect framework, run tests, parse failures, suggest fixes |
| Agentic loop | `/agent <task>` | Plan → edit → test → commit autonomous loop |
| Code review | `/review [path]` | LLM review of diff, suggest improvements |

**Files:**
- `src/tools/git.rs` — extend with `stage_all`, `generate_commit_msg`, `push`, `branch_create`
- `src/tools/pr.rs` — new: PR creation via `gh` CLI or git push + URL
- `src/tui/mod.rs` — add `/commit`, `/pr`, `/test`, `/agent`, `/review` slash commands
- `src/agent/coding.rs` — new: agentic coding loop (plan/edit/test/commit)

---

## Tier 2: Hermes Bridge (Optional) 🟡 SCAFFOLDED

**Goal:** Two-way sync with Hermes — OpenShark reads Hermes memory, Hermes reads OpenShark skills.

```bash
openshark hermes status      # Show bridge status
openshark hermes sync        # Pull memories from Hermes
openshark hermes push        # Push skills to Hermes
openshark hermes bridge      # Start real-time sync daemon
```

**Config (optional, off by default):**
```toml
[integrations.hermes]
enabled = false
hermes_home = "~/.hermes"
sync_interval_seconds = 300
pull_memories = true
push_skills = true
```

**Files:**
- `src/integrations/mod.rs` — integration registry
- `src/integrations/hermes.rs` — Hermes bridge
- `src/config/mod.rs` — add `integrations: IntegrationsConfig`

---

## Tier 3: OpenClaw / OpenCode / Claw-Code Interop (Optional) ✅ COMPLETE

**Goal:** Delegate to other agents, stream results back. OpenShark as conductor.

```bash
openshark delegate claw "refactor auth module"      # Delegate to Claw Code
openshark delegate opencode "fix bug #42"           # Delegate to OpenCode
openshark delegate claude "write tests for src/lib.rs"  # Delegate to Claude Code
```

**Behavior:**
- Detects if tool is installed (`which claw`, `which opencode`, etc.)
- Spawns process, streams stdout/stderr back to TUI in real-time
- Captures result, stores in OpenShark memory
- If tool not installed: suggests install command, does not fail

**Files:**
- `src/integrations/claw.rs` — Claw Code delegation
- `src/integrations/opencode.rs` — OpenCode delegation
- `src/integrations/claude.rs` — Claude Code delegation
- `src/integrations/registry.rs` — unified delegation registry

---

## Tier 4: Platform Expansion 🟡 PARTIAL

**Goal:** GitHub-native integration, richer Discord commands, web API scaffold.

| Feature | Status | Description |
|---------|--------|-------------|
| GitHub MCP native | 📋 | Built-in GitHub tools (issues, PRs, repos) without external MCP |
| Discord slash commands | 📋 | `/swarm`, `/code`, `/review` slash commands |
| Web API scaffold | ✅ | HTTP REST API + WebSocket streaming (feature-gated) |
| SSE streaming endpoint | ✅ | WebSocket streaming at /ws/v1/chat and /ws/v1/agent |

**Files:**
- `src/capabilities/github.rs` — native GitHub REST API tools
- `src/gateway/discord_slash.rs` — slash command handlers
- `src/api/mod.rs` — optional web API (feature-gated)

---

## Tier 5: The Synthesis Engine 📋 NOT STARTED

**Goal:** Meta-learning across harnesses. OpenShark learns which agent performs best per task type.

```bash
openshark synthesis "fix the auth bug"   # Auto-routes to best agent
openshark synthesis --compare "refactor" # Run on all available agents, compare
```

**Behavior:**
- Tracks: success rate, latency, code quality, test pass rate per agent per task type
- Auto-routes new tasks to historically best performer
- Falls back to local LLM if external agents unavailable
- All tracking local-only, no external telemetry

**Files:**
- `src/synthesis/mod.rs` — meta-learning engine
- `src/synthesis/router.rs` — task → agent routing
- `src/synthesis/tracker.rs` — outcome tracking

---

## Implementation Order

1. **Tier 1** — Git agent ✅ COMPLETE
2. **Tier 3** — Delegation ✅ COMPLETE
3. **Tier 4** — Web API ✅ COMPLETE | GitHub + Discord 📋
4. **Tier 2** — Hermes bridge 🟡 SCAFFOLDED (needs real sync)
5. **Tier 5** — Synthesis engine 📋 NOT STARTED

Each tier is independently shippable. All integrations behind feature flags + config gates.
