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
watch -n1 'ps -o rss= -p $(pgrep openshark)'

# Trigger stall (disconnect network mid-stream)
# Verify RSS drops after watchdog fires

# Type "stop" mid-stream
# Verify RSS drops immediately
```

## Remaining Work

- [ ] Convert `unbounded_channel` to bounded `channel(100)` for backpressure
- [ ] Add `Arc` to `messages: Vec<ChatMessage>` (display history)
- [ ] Consider `parking_lot::Mutex` for fine-grained branch locking
