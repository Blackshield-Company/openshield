# Memory Leak Fixes Applied

## Changes Made

### 1. Arc-ified Model Messages
- `model_messages: Vec<Message>` → `model_messages: Arc<Vec<Message>>`
- Cheap clones (8-byte refcount bump vs 50KB+ deep copy)
- Mutations use `Arc::make_mut()` for copy-on-write

### 2. JoinHandle Storage + Abort
- Added `stream_task: Option<JoinHandle<()>>` to App struct
- Store handle at all 4 spawn sites:
  - `auto_continue()` (mod.rs:1486)
  - `execute_approved_tool_task()` (mod.rs:1947)
  - Control word restart (mod.rs:3889)
  - `process_user_input()` (mod.rs:4157)
  - stream.rs restart (stream.rs:376)
- Abort on stall watchdog (mod.rs:1600)
- Abort on cancel/stop/abort/pause (mod.rs:3911)

### 3. Aggressive Truncation
- `MAX_MODEL_MESSAGES`: 100 → 50
- `KEEP_LAST`: 80 → 40
- `MAX_MESSAGES`: 300 → 200
- `KEEP_LAST`: 200 → 150

## Expected Impact

| Scenario | Before | After |
|----------|--------|-------|
| Idle RAM growth | Unbounded (orphaned tasks) | Bounded (tasks aborted) |
| Message clone cost | 50KB+ per spawn | 8 bytes per spawn |
| Stall recovery | Task keeps running | Task killed |
| Cancel recovery | Task keeps running | Task killed |

## Testing

```bash
# Build
cargo build --release

# Run TUI, send 20+ messages
# Monitor RSS in another terminal:
watch -n1 'ps -o rss= -p $(pgrep openshield)'

# Trigger stall (disconnect network mid-stream)
# Verify RSS drops after watchdog fires

# Type "stop" mid-stream
# Verify RSS drops immediately
```

## Remaining Work

- [ ] Convert `unbounded_channel` to bounded `channel(100)` for backpressure
- [ ] Add `Arc` to `messages: Vec<ChatMessage>` (display history)
- [ ] Consider `parking_lot::Mutex` for fine-grained branch locking

---

## Round 2 — The Code Index Was Eating Everything (2026-09-07)

Repro evidence (`/tmp/oshield-rss.log`): idle TUI sat flat at **29 MB RSS for
10 minutes**, then RSS ratcheted past **2 GB** and VSZ ballooned to **27 GB**
in a sawtooth pattern. The 10-minute delay exactly matched
`CodeIndex::spawn_background_refresh` — it sleeps two 5-min intervals, then
calls `rebuild()` → `build_repo_map(cwd)` every 5 minutes.

### Root Causes (all in the repo-map/code-index path)

1. **Unbounded walk** — scanned the *entire* cwd tree regardless of what it
   was (home directory, random folder full of ISOs, anything).
2. **`read_to_string` on every file** — no size cap, no type filter. Multi-GB
   binaries were read fully into RAM per scan cycle (the RSS sawtooth).
3. **Regexes compiled per file** — up to 9 regex compilations × thousands of
   files × every 5 minutes. This was the CPU burn.
4. **Full `DELETE` + re-`INSERT` of all symbols every cycle** — SQLite churn
   even when nothing changed.
5. `last_refresh` meta stored the symbol *count*, not a timestamp (latent bug).

### Fixes

- `repo_map.rs`
  - Walk bounded: max depth 10, max 5 000 files, files > 256 KiB never read.
  - Contents read only for languages with symbol patterns (binaries skipped).
  - All regexes compiled once per process in a `LazyLock` static.
  - `RepoMap.fingerprint` (file count + len/mtime rolling hash) added.
  - `looks_like_project_root()` gate (.git, Cargo.toml, package.json, …).
- `code_index.rs`
  - `rebuild()` refuses to scan non-project roots.
  - Skips the DELETE+INSERT rewrite when the fingerprint is unchanged.
  - `last_refresh` now stores a real timestamp; symbol count in `symbol_count`.
  - SQLite `cache_size` capped at ~2 MB, WAL journal mode.
- `tui/mod.rs` + `config`
  - Background refresh only spawns when cwd is a project root.
  - New config toggle: `code_index_enabled = false` disables it entirely.
  - `OPENSHIELD_INDEX_REFRESH_SECS` env override (min 10s) for testing.
- Regression tests: scan bounds, fingerprint change detection,
  project-root gate, rebuild skip-on-unchanged.

### Verification

| Scenario | Before | After |
|----------|--------|-------|
| Idle from `$HOME` | 29 MB → multi-GB RSS, VSZ → 27 GB | 6 MB flat, 0.2 % CPU |
| 546 MB bait dir (500 MB bin + 50 MB JSON + 2 000 sources), refresh every **15 s** | would OOM | peaked 66 MB, settled ~50 MB, VSZ constant |

`cargo test`: 511 passed, 0 failed.
