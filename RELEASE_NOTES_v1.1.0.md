# OpenShark v1.1.0 Release Notes

**Release Date:** 2026-06-02

## What's New

### Command Palette (`Ctrl+P`)
Fuzzy-searchable command overlay with 16 built-in commands. Type `/` in the input box or hit `Ctrl+P` anywhere. Navigate with ↑/↓, filter with typing, Enter to execute, Esc to close. No more memorizing slash commands.

### Session Bookmarks (`Ctrl+Shift+B`)
Save and restore named checkpoints of your session state. Perfect for branching conversations or preserving important context before experimenting. Persistent JSON storage per session in `~/.config/openshark/bookmarks/`.

### Inline Image Display
Pasted images now show rich metadata — format, dimensions, file size — plus an ASCII art placeholder box. Supports PNG, JPEG, GIF, WebP, and BMP with header-based dimension detection.

### Streaming Markdown Renderer
Assistant messages now render inline markdown in real-time: **bold**, *italic*, `code`, [links](url), ~~strikethrough~~. Applied to all non-code-block content while streaming.

### Performance Improvements
- **Connection pooling** — `reqwest` client with HTTP/2, TCP keepalive, and 10 idle connections per host
- **Async cache persistence** — Disk writes spawned via `tokio::spawn`, eliminating fs blocking on the hot path
- **Cache key optimization** — Eliminated double JSON serialization by reusing request body for cache keys

## Stats
- **Tests:** 363 (up from 337)
- **Warnings:** 0 (down from 40)
- **Version:** 1.1.0

## Upgrade Notes
No breaking changes. Existing configs and sessions are fully compatible.
