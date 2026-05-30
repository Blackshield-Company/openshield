---
name: debugging
description: Debugging strategies and techniques
triggers:
  - debug
  - debugging
  - breakpoint
  - trace
  - log
  - error
  - panic
  - stack trace
  - gdb
  - lldb
tags:
  - debugging
  - troubleshooting
---

# Debugging Guide

## Systematic Approach
1. **Reproduce** — Can you make it fail consistently?
2. **Isolate** — What's the minimal case that triggers it?
3. **Hypothesize** — What could cause this? List possibilities.
4. **Test** — Prove or disprove each hypothesis.
5. **Fix** — Make the smallest change that fixes it.
6. **Verify** — Confirm the fix works and nothing else broke.

## Logging
- Use `tracing` in Rust — structured, levels, spans
- `info!` for high-level flow, `debug!` for details, `trace!` for spam
- Always include context: `tracing::info!(user_id = %id, "login attempt")`
- Don't log secrets, tokens, or PII

## Rust Debugging
- `RUST_BACKTRACE=1` for backtraces
- `RUST_LOG=debug` for crate-level logging
- `dbg!(&variable)` — quick print debugging
- `cargo expand` to see macro expansions
- `clippy` catches common bugs before they happen

## Common Bugs
- Off-by-one errors: check loop bounds and array indices
- Race conditions: look for shared mutable state
- Lifetime issues: who owns what, and for how long?
- Resource leaks: files, connections, locks not released
- Async bugs: blocking in async context, wrong mutex type

## Tools
- `gdb` / `lldb` — native debugger
- `valgrind` — memory debugging (C/C++)
- `rr` (Mozilla) — record and replay debugging
- `perf` — performance profiling
- `flamegraph` — visualize hotspots
