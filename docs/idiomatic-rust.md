# Idiomatic Rust Guidelines for `qam`

## Core Principles

1. **Zero Runtime Dependencies**: Never introduce external binary runtime dependencies (no `ffmpeg`, `ffprobe`, `libav*`, or `curl`).
2. **Explicit Error Handling**:
   - Use `QamError` via `thiserror` for typed errors.
   - Do NOT use `.unwrap()` or `.expect()` in library code. Bubble errors up with `?`.
   - Preserve error classifications (`auth`, `network-refused`, `network-timeout`, `not-found`).
3. **Async Rules**:
   - Use Tokio for asynchronous I/O and tasks.
   - Always apply timeouts on network I/O operations (`tokio::time::timeout`).
   - Cleanly teardown RTSP sessions (`session.teardown().await`) on exit or error.
4. **Clean Shutdown**:
   - Handle SIGINT and SIGTERM gracefully via `src/signals.rs`.
   - On interruption, finalize media containers cleanly and exit with code 130.
5. **Quality Gates**:
   - Code must pass `cargo fmt --check`.
   - Code must pass `cargo clippy --all-targets -- -D warnings`.
   - All tests must pass with `cargo test`.
