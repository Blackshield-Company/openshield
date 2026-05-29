# OpenShark Roadmap

## Phase 1: Core Engine (Weeks 1-2)
**Goal:** Chat with any model, persistent memory, basic tools.
**Status: ✅ COMPLETE** — v0.1.0 shipped with chat, SQLite memory, fs/terminal tools

## Phase 2: Coding Depth (Weeks 3-5)
**Goal:** Match Claw Code / Claude Code for coding tasks.

| Week | Milestone | Deliverable |
|------|-----------|-------------|
| 3 | Multi-file editing | Diff-based file modifications |
| 3 | Codebase search | ripgrep integration, symbol search |
| 4 | LSP integration | Go-to-definition, type info, diagnostics |
| 4 | Git integration | status, diff, commit, branch, checkout |
| 5 | Test runner | Auto-detect test framework, run tests |
| 5 | Refactor engine | Extract function, rename symbol, etc. |

**Success Criteria:** Can build a feature end-to-end without leaving OpenShark.

## Phase 3: Real Memory (Weeks 6-7)
**Goal:** Surpass Hermes memory with semantic search and cross-session context.

| Week | Milestone | Deliverable |
|------|-----------|-------------|
| 6 | Vector embeddings | sqlite-vec or similar for semantic search |
| 6 | Memory hierarchy | session → project → global context layers |
| 7 | Context injection | Auto-inject relevant past sessions |
| 7 | Natural queries | "What did we do about auth?" → instant answer |

**Success Criteria:** 3-week-old context recalled in < 2 seconds.

## Phase 4: Agent Autonomy (Weeks 8-9)
**Goal:** Exceed OpenCode/OMO agent control.

| Week | Milestone | Deliverable |
|------|-----------|-------------|
| 8 | Auto-tool detection | Model suggests tools, user approves |
| 8 | Agentic loop | plan → execute → verify → iterate |
| 9 | Parallel execution | Multiple tools concurrently |
| 9 | Error recovery | Retry, fallback, escalation logic |

**Success Criteria:** "Fix the bug" → finds, fixes, tests, commits without guidance.

## Phase 5: Speed & Polish (Weeks 10-12)
**Goal:** Faster and more responsive than any harness.

| Week | Milestone | Deliverable |
|------|-----------|-------------|
| 10 | Streaming | Real-time token streaming |
| 10 | Async tools | Non-blocking tool execution |
| 11 | Connection pool | Reuse connections, reduce latency |
| 11 | Response cache | Cache common responses |
| 12 | TUI polish | Ratatui interface, themes, keybindings |

**Success Criteria:** First token in < 500ms, tool results in < 1s.

## Success Metrics

| Phase | Metric |
|-------|--------|
| 1 | Can chat with any model, memory persists |
| 2 | Routing beats random selection by 30% |
| 3 | Feature build without leaving TUI |
| 4 | Self-improvement measurably better |
| 5 | 100+ GitHub stars, 10+ contributors |
