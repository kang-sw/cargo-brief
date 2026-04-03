# LSP cfg(unix) Gate Removal (Phase 3)

## Context

**Ticket:** `260328-refactor-lsp-cross-platform-ipc` Phase 3.

**Goal:** Remove `#[cfg(unix)]` from the `lsp` module so it compiles on all
platforms. Replace the remaining Unix-specific code (libc poll on ra stdout)
with a cross-platform thread+channel pattern.

**Key ticket decisions:**
- Phases 1 & 2 already abstracted IPC (`ipc/`) and process management
  (`process/`). Phase 3 addresses the remaining blockers.
- Runtime testing deferred to `260326-feat-lsp-windows-support`.
- `notify` crate is already cross-platform; only the Cargo.toml gate needs
  changing.

**What remains Unix-specific (verified from code):**
1. `transport.rs` L4: `use std::os::unix::io::{AsRawFd, RawFd}` and
   L63-64: `stdout_raw_fd()` method returning `RawFd`.
2. `daemon.rs` L12: `use super::ipc::poll_retry` and L269-274, L349-354:
   direct use of `libc::pollfd` + `libc::POLLIN` + `poll_retry()` to poll
   ra stdout.
3. `ipc/mod.rs` L18-19: `poll_retry` re-export (Unix-only).
4. `Cargo.toml`: `libc` is unconditional (should be `cfg(unix)`); `notify`
   is `cfg(unix)` (should be unconditional).
5. `lib.rs` L5-6, L22-23: `#[cfg(unix)]` on `pub mod lsp` and
   `pub fn run_lsp_command`.

**Design: thread+channel for ra stdout (encapsulated in RaTransport)**

Replace `libc::poll()` + `poll_retry()` with a background reader thread.
The channel receiver is stored **inside** `RaTransport`, making the switch
transparent to callers. `query.rs` and `send_request_and_wait()` work
unchanged — they call `read_message()` which reads from the channel after
the thread is spawned.

After `send_initialize()` (which uses direct `read_message()`), the daemon
calls `transport.spawn_reader_thread()`. This:
1. Takes `self.stdout` (the `BufReader<ChildStdout>`)
2. Spawns a thread doing blocking reads, sending results over `mpsc::channel`
3. Stores the `Receiver` in `self.ra_rx`

After spawning:
- `read_message()` → reads from channel (blocking `recv()`)
- New `try_read_message()` → `try_recv()` (non-blocking, for drain)
- New `read_message_timeout(dur)` → `recv_timeout()` (for wait_for_ready)
- `send_request_and_wait()` → unchanged (calls `read_message()` which now
  reads from channel — during queries, notifications consumed inline, same
  as current behavior)
- `has_buffered_data()` → returns false (channel handles buffering)

This eliminates all `libc::poll`, `RawFd`, `AsRawFd` from daemon.rs and
transport.rs while keeping query.rs completely unchanged.

`shutdown_ra()` is simplified to just send shutdown request + exit
notification without reading responses. This is already documented as
acceptable behavior ("ra handles exit regardless of shutdown ACK"). The
reader thread exits when ra closes stdout.

**Scope boundary:** This phase removes compilation blockers only. Runtime
concerns (Windows `file://` URI backslash normalization in `watcher.rs` and
`daemon.rs::send_initialize()`) are deferred to the Windows support ticket.

## Relevant Files

- `src/lsp/transport.rs` — `RaTransport` struct. Changes: make stdout
  `Option`, add `ra_rx: Option<Receiver>`, add `spawn_reader_thread()`,
  `try_read_message()`, `read_message_timeout()`. Remove `stdout_raw_fd()`.
- `src/lsp/daemon.rs` — Main daemon loop. Changes: call
  `transport.spawn_reader_thread()`, rewrite `drain_ra_messages()` to use
  `try_read_message()`, rewrite `wait_for_ready()` to use
  `read_message_timeout()`, simplify `shutdown_ra()`.
- `src/lsp/query.rs` — No changes needed. Calls `send_request_and_wait()`
  which transparently reads from channel after thread is spawned.
- `src/lsp/ipc/mod.rs` — Remove `poll_retry` re-export (L18-19).
- `src/lib.rs` — Remove `#[cfg(unix)]` gates (L5-6, L22-23).
- `Cargo.toml` — Move `libc` to `cfg(unix)`, move `notify` to unconditional.
- `src/lsp/watcher.rs` — No changes needed (already cross-platform code).
  Note: `file://` URI generation uses `path.display()` which produces
  backslashes on Windows — known runtime concern, deferred.
- `src/lsp/mod.rs` — No changes needed.
- `src/lsp/client.rs` — No changes needed.
- `src/main.rs` — No changes needed (already has no cfg gates on lsp calls).

## Conventions (verified from code)

- **Thread spawning pattern** (daemon.rs L403-410): Background thread for ra
  stderr uses `std::thread::spawn` with a `move` closure. Same pattern for
  the reader thread.
- **Channel pattern** (watcher.rs L25): `mpsc::channel()` for watcher events.
  `try_recv()` in main loop (daemon.rs L577). Same pattern for ra messages.
- **Error on channel close**: `recv_timeout()` returns `RecvTimeoutError::Disconnected`
  when the sender drops (thread exits). Map this to "ra stdout closed".

## Implementation Steps

### Step 1: Modify `transport.rs` — add reader thread support

**Struct changes:**
```rust
pub struct RaTransport {
    stdin: ChildStdin,
    stdout: Option<BufReader<ChildStdout>>,
    ra_rx: Option<Receiver<Result<serde_json::Value>>>,
    next_id: i32,
}
```

**Remove:**
- `use std::os::unix::io::{AsRawFd, RawFd};`
- `pub fn stdout_raw_fd(&self) -> RawFd` method

**Modify constructor:** wrap stdout in `Some(...)`, init `ra_rx: None`.

**Extract `read_one_message()`** as a private static method that reads headers
+ body from any `impl BufRead`. This is the core read logic extracted from
the current `read_message()` + `read_headers()`. The thread and `read_message()`
both use it.

**Modify `read_message()`:**
```rust
pub fn read_message(&mut self) -> Result<serde_json::Value> {
    if let Some(stdout) = &mut self.stdout {
        Self::read_one_message(stdout)
    } else if let Some(rx) = &self.ra_rx {
        match rx.recv() {
            Ok(result) => result,
            Err(_) => bail!("rust-analyzer stdout closed"),
        }
    } else {
        bail!("transport has no reader")
    }
}
```

**Modify `has_buffered_data()`:**
```rust
pub fn has_buffered_data(&self) -> bool {
    self.stdout.as_ref().is_some_and(|s| !s.buffer().is_empty())
}
```

**New methods:**
```rust
/// Spawn a background thread to read LSP messages from ra stdout.
/// After this, read_message() reads from the channel (blocking).
/// The thread exits when ra closes stdout or the receiver is dropped.
pub fn spawn_reader_thread(&mut self) {
    let mut stdout = self.stdout.take().expect("reader thread already spawned");
    let (tx, rx) = mpsc::channel();
    self.ra_rx = Some(rx);
    std::thread::spawn(move || {
        loop {
            match Self::read_one_message(&mut stdout) {
                Ok(msg) => {
                    if tx.send(Ok(msg)).is_err() {
                        break; // receiver dropped
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break;
                }
            }
        }
    });
}

/// Non-blocking read. Returns Ok(Some(msg)) if available, Ok(None) if empty,
/// or the error from the reader thread.
/// Only available after spawn_reader_thread().
pub fn try_read_message(&self) -> Result<Option<serde_json::Value>> {
    let rx = self.ra_rx.as_ref().expect("reader thread not spawned");
    match rx.try_recv() {
        Ok(result) => result.map(Some),
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => bail!("rust-analyzer stdout closed"),
    }
}

/// Read with timeout. Returns Ok(Some(msg)) if available, Ok(None) on timeout,
/// or the error from the reader thread.
/// Only available after spawn_reader_thread().
pub fn read_message_timeout(&self, timeout: Duration) -> Result<Option<serde_json::Value>> {
    let rx = self.ra_rx.as_ref().expect("reader thread not spawned");
    match rx.recv_timeout(timeout) {
        Ok(result) => result.map(Some),
        Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
        Err(mpsc::RecvTimeoutError::Disconnected) => bail!("rust-analyzer stdout closed"),
    }
}
```

Add `use std::sync::mpsc::{self, Receiver};` and `use std::time::Duration;`
to imports.

- Delegation: main agent
- Depends on: nothing

### Step 2: Modify `daemon.rs` — replace poll with transport methods

**Remove:**
- `use super::ipc::poll_retry;` import (L12)

**Modify `run_daemon()`:**
- After `send_initialize()` succeeds and before IPC setup, call:
  `transport.spawn_reader_thread();`

**Rewrite `drain_ra_messages()`:**
```rust
fn drain_ra_messages(
    transport: &mut RaTransport,
    ra_status: &mut RaStatus,
    active_progress: &mut HashSet<String>,
    had_progress: &mut bool,
    start_time: Instant,
) -> bool {
    let mut any_read = false;
    loop {
        match transport.try_read_message() {
            Ok(Some(msg)) => {
                any_read = true;
                handle_ra_message(&msg, transport, ra_status, active_progress, had_progress);
            }
            Ok(None) => break,
            Err(e) => {
                eprintln!("[lsp-daemon] ra stdout read error: {e}");
                break;
            }
        }
    }
    check_no_progress_fallback(ra_status, *had_progress, start_time);
    any_read
}
```

Note: return type changes from `Result<bool>` to `bool`. Signature otherwise
matches the current one (same parameters), so `handle_ra_message()` is
unchanged.

**Rewrite `wait_for_ready()`:**
Replace the inner poll+read block (L346-370) with:
```rust
// Poll ra stdout — shorter interval during settle for responsiveness
let poll_timeout = if ready_since.is_some() { 100 } else { 500 };
match transport.read_message_timeout(Duration::from_millis(poll_timeout))? {
    Some(msg) => {
        handle_ra_message(&msg, transport, ra_status, active_progress, had_progress);
    }
    None => {
        check_no_progress_fallback(ra_status, *had_progress, start_time);
        continue;
    }
}
```

Remove `has_buffered_data()` checks from both functions — channel handles
buffering internally.

**Simplify `shutdown_ra()`:**
```rust
fn shutdown_ra(transport: &mut RaTransport) {
    let _ = transport.send_request("shutdown", serde_json::Value::Null);
    let _ = transport.send_notification("exit", serde_json::Value::Null);
}
```

The read loop is removed. The reader thread has responses in its channel;
we don't need to process them during shutdown. The thread will exit when
ra closes stdout after receiving exit.

**Update call sites in `run_daemon()`:**
- Remove `?` from `drain_ra_messages()` calls (no longer returns Result).
  Change `if let Err(e) = drain_ra_messages(...)` to just
  `drain_ra_messages(...)` (or use the bool return for logging).

- Delegation: main agent
- Depends on: Step 1

### Step 3: Remove `poll_retry` re-export from `ipc/mod.rs`

Remove lines 17-19:
```rust
// Re-export poll_retry for daemon.rs ra-stdout polling
// (Unix-only, will be replaced in Phase 3 with transport abstraction)
#[cfg(unix)]
pub(super) use unix::poll_retry;
```

- Delegation: main agent
- Depends on: Step 2

### Step 4: Update `Cargo.toml` dependencies

- Move `libc = "0.2"` from `[dependencies]` to `[target.'cfg(unix)'.dependencies]`
  (alongside the existing `notify` entry — but `notify` moves out).
- Move `notify = "6"` from `[target.'cfg(unix)'.dependencies]` to
  `[dependencies]` (it's cross-platform).

- Delegation: main agent
- Depends on: Step 3

### Step 5: Remove `#[cfg(unix)]` gates from `lib.rs`

Remove `#[cfg(unix)]` from:
- L5: `pub mod lsp;`
- L22: `pub fn run_lsp_command(...)`

- Delegation: main agent
- Depends on: Step 4

### Step 6: Verify compilation

1. `cargo test` — all existing tests pass.
2. `cargo clippy` — no new warnings.
3. `cargo check --target x86_64-pc-windows-msvc` — may still fail on
   `ring`/`tree-sitter` C build scripts (environment limitation). If so,
   note as known limitation.

- Delegation: main agent
- Depends on: Step 5

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| `transport.rs` | Post-impl (existing test + integration) | `content_length_format` test is platform-independent. `spawn_reader_thread` is hard to unit-test (needs child process). Covered by integration tests (full LSP pipeline). |
| `daemon.rs` | Post-impl (existing integration tests) | Main loop restructured but behavior identical. 232+ integration tests cover the full pipeline. Unit tests for `process_ra_notification` unchanged. |
| `ipc/mod.rs` | N/A | Pure re-exports, removing one line. |
| `Cargo.toml` | Post-impl (cargo check) | Dependency gating verified by compilation. |
| `lib.rs` | Post-impl (cargo check) | Gate removal verified by compilation. |

**Key verification scenarios:**
- `cargo test` passes (all tests)
- `cargo clippy` clean (no new warnings)
- `cargo check --target x86_64-pc-windows-msvc` (best-effort, may fail on C deps)

## Success Criteria

1. No `#[cfg(unix)]` on `pub mod lsp` or `pub fn run_lsp_command` in `lib.rs`.
2. No `libc::*` or `std::os::unix::*` usage in `daemon.rs` or `transport.rs`.
3. `poll_retry` no longer re-exported from `ipc/mod.rs`.
4. `libc` dependency gated to `cfg(unix)` in Cargo.toml.
5. `notify` dependency unconditional in Cargo.toml.
6. `cargo test` passes with no behavioral changes.
7. `query.rs` unchanged — `send_request_and_wait()` transparently uses channel.
