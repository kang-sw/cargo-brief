---
title: "Fix LSP daemon lifecycle: setsid + blocking touch"
status: done
completed: 2026-03-29
plans:
  phase-1-and-2: 2026-03/29-1500.lsp-daemon-lifecycle
related:
  260326-feat-lsp-daemon: original daemon bootstrap
  260327-feat-lsp-indexing-status: indexing status tracking
  260328-refactor-lsp-cross-platform-ipc: cross-platform IPC
---

# Fix LSP daemon lifecycle: setsid + blocking touch

## Problem

Two related issues with the LSP daemon lifecycle:

1. **Daemon dies when parent shell exits.** `configure_daemon_spawn()` uses
   `process_group(0)` which calls `setpgid(0,0)` — new process group but
   same session. When the terminal/shell session closes, SIGHUP can propagate
   to processes in the same session. The daemon needs `setsid()` to create a
   new session entirely.

2. **`touch` returns before indexing completes.** Currently `touch` only waits
   for the readiness indicator (ra initialize done), then returns. If ra crashes
   during indexing, the user doesn't find out until the next query command. The
   error is invisible and confusing ("not running" on subsequent commands).

## Phase 1: `setsid()` fix (Unix)

Replace `cmd.process_group(0)` with `unsafe { cmd.pre_exec(|| setsid()) }` in
`src/lsp/process/unix.rs`. `setsid()` implies new process group, so
`process_group(0)` is no longer needed.

**Success criteria:** Daemon survives parent shell exit.

## Phase 2: Blocking `touch` with indexing wait

### Design decisions

- **`touch` blocks by default** until ra finishes indexing. `--no-wait` flag
  skips the wait (fire-and-forget, original behavior).
- **New `DaemonRequest::WaitForReady`** variant. Daemon-side: calls
  `wait_for_ready()` with **no timeout** (unlimited budget). The assumption is
  that ra will not freeze during indexing — it either completes or crashes.
- **Progress indicator on client side:** While waiting, print to stderr every
  3 seconds: first line `"Indexing . . "`, then append one ` .` per tick.
  No spinner (LLM-hostile). Example: `Indexing . . . . . .`
- **Error reporting:** If the daemon returns `DaemonResponse::Error`, include
  the last 20 lines of `lsp.log` in the error output (same pattern as
  `wait_for_daemon()` early-death detection).
- **Client-side timeout for `send_command`:** Use `Duration::MAX` (or
  `i32::MAX` ms in poll). The `ipc::send_command` already clamps to
  `c_int::MAX` via `try_into().unwrap_or(c_int::MAX)`.

### Changes required

1. **`cli.rs`**: Add `--no-wait` flag to `LspCommand::Touch`.
2. **`protocol.rs`**: Add `DaemonRequest::WaitForReady` variant.
3. **`daemon.rs`**: Handle `WaitForReady` — call `wait_for_ready()` with
   unlimited timeout. If already `Ready`, return immediately. Return
   `DaemonResponse::Ok` on success, `DaemonResponse::Error` on ra crash.
   `WaitForReady` should be treated like a query for the `is_query` gate
   (it needs to wait for ready).
4. **`mod.rs`**: Update `cmd_touch()` — after `ensure_daemon()`, send
   `WaitForReady` request (unless `--no-wait`). Print progress dots to
   stderr every 3 seconds while waiting. On error, read and append
   `lsp.log` tail.
5. **`run_lsp_command()`**: Pass `no_wait` flag through to `cmd_touch()`.

### Progress dot implementation

Client-side, not daemon-side. The `send_command()` call blocks on poll, so
progress dots need a separate thread or a polling loop:
- Spawn a thread that prints ` .` to stderr every 3 seconds.
- When `send_command()` returns, signal the thread to stop and print newline.
- Alternatively: use a custom poll loop in `cmd_touch()` instead of
  `send_command()` — poll with 3s timeout, print dot on timeout, retry.
  This avoids threading but requires duplicating some IPC logic.

Preferred: **thread approach** — simpler, no IPC duplication.

**Success criteria:** `cargo brief lsp touch` blocks until indexing completes,
shows progress dots, and surfaces errors with log context.

### Result (4137cb8) - 26-03-29

**Phase 1 + Phase 2 implemented together.**

Phase 1: `process_group(0)` replaced with `pre_exec { setsid() }` in
`src/lsp/process/unix.rs`.

Phase 2: `cmd_touch` blocks by default. Changes:
- `DaemonRequest::WaitForReady` variant added to protocol.
- Daemon main loop: `is_query` boolean replaced with per-variant
  `Option<Duration>` `wait_timeout` match. `WaitForReady` gets
  `Duration::MAX`, queries get `ready_timeout`.
- `handle_request()` returns `DaemonResponse::Ok { message: "rust-analyzer ready" }`.
- `LspCommand::Touch` changed to `Touch { no_wait: bool }`.
- `cmd_touch()`: blocking mode spawns dot thread (3s interval), sends
  `WaitForReady` with `Duration::MAX`, shows `lsp.log` tail on error.
- `read_log_tail` made `pub(super)` for `mod.rs` access.
- Windows `send_command`: `checked_add` prevents `Instant` overflow panic
  with `Duration::MAX`.
- Added forward-looking `RaStatus::Stopped` guard in `wait_for_ready()`.
- Roundtrip test for `WaitForReady` added.

**Deviations:** None. All plan steps followed as designed.

**Key finding:** `Instant::now() + Duration::MAX` panics on Windows — must
use `checked_add` with fallback. Unix `poll()` clamps safely via
`try_into().unwrap_or(c_int::MAX)`.
