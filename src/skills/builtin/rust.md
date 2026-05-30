---
name: rust
description: Rust programming best practices and patterns
triggers:
  - rust
  - cargo
  - rustc
  - tokio
  - async rust
  - lifetime
  - borrow checker
tags:
  - rust
  - systems
---

# Rust Programming Guide

When helping with Rust code, follow these principles:

## Error Handling
- Use `anyhow` for application code — `Result<T>` and `Context`
- Use `thiserror` for library code — derive `Error` with `#[error("...")]`
- Never use `.unwrap()` or `.expect()` in production code
- Chain context with `.with_context(|| format!("..."))`

## Async Patterns
- Prefer `tokio::sync::mpsc` over `std::sync::mpsc` in async contexts
- Use `tokio::spawn` for concurrent tasks
- Remember: `Mutex` in async — use `tokio::sync::Mutex`, not `std::sync::Mutex`
- `RwLock` — same rule: `tokio::sync::RwLock` for async

## Ownership & Lifetimes
- Clone is cheap for `Arc<T>` and `String` — don't over-optimize
- Use `&str` for function args when you don't need ownership
- `Cow<'_, str>` for "maybe borrowed, maybe owned" strings
- When the borrow checker fights you, consider `Arc<str>` or `Arc<String>`

## Common Pitfalls
- `RefCell` is NOT `Send`/`Sync` — can't share across threads
- `rusqlite::Connection` uses `RefCell` internally — same problem
- `HashMap` iteration order is non-deterministic — use `BTreeMap` if order matters
- `PathBuf` != `String` — use `.display()` for printing, `.to_string_lossy()` for conversion

## Testing
- Use `tokio::test` for async tests
- `#[should_panic]` for error path testing
- `tempfile` crate for temp files in tests
