# LSP IPC Abstraction Layer (Phase 1)

## Context

**Ticket:** `260328-refactor-lsp-cross-platform-ipc` Phase 1.

**Goal:** Extract all FIFO-specific IPC code from `client.rs` and `daemon.rs` into
a new `src/lsp/ipc/` module with Unix and Windows implementations. Unix behavior
must be unchanged (pure refactor). Windows implements atomic-rename file protocol
per ticket design.

**Key ticket decisions:**
- Unix stays on FIFOs — no change to existing mechanism.
- Windows uses atomic-rename file protocol: `write(tmp) → rename(ready)` for
  both request and response. `LockFileEx` for client serialization.
- IPC files live in `<target_dir>/cargo-brief-lsp/<hash>/` (same as today).
- Follow the `process/` module pattern: `cfg`-gated re-exports, no Rust traits.
  (Deviation from ticket Phase 1 text which says "IPC trait" — the struct + cfg
  pattern is simpler and consistent with the already-implemented Phase 2.)

**Scope boundary:** This phase covers client↔daemon IPC only. The `RaTransport`
stdout polling (`poll_retry` on ra's stdout fd in `drain_ra_messages()` and
`wait_for_ready()`) remains Unix-specific in `daemon.rs`. Phase 3 will address
that when removing the outer `#[cfg(unix)]` gate.

## Relevant Files

- `src/lsp/client.rs` — Contains IPC primitives (`create_fifo`, `flock_exclusive`,
  `poll_retry`, `set_nonblocking`) and `send_command()`. Also contains non-IPC
  logic: `daemon_dir()`, `ensure_daemon()`, `spawn_daemon()`, `wait_for_daemon()`,
  `cleanup_daemon_files()`, `short_hash()`.
- `src/lsp/daemon.rs` — Main loop uses IPC: FIFO creation (L446-449), FIFO open
  with `O_RDWR`/`O_NONBLOCK` (L454-465), poll req_fd (L489-507), nonblock toggle
  (L498-507), read/write messages on FIFOs. Also uses `poll_retry` for ra stdout
  (L271) — this stays Unix-specific.
- `src/lsp/protocol.rs` — `read_message`/`write_message` (pure serialization,
  platform-independent). Tests use `UnixStream::pair()` — needs `#[cfg(unix)]`.
- `src/lsp/transport.rs` — `RaTransport` with `stdout_raw_fd()` (Unix-only).
  Not modified in this phase.
- `src/lsp/mod.rs` — Imports from `client`. Will add `mod ipc` and update imports.
- `src/lsp/process/mod.rs` — Pattern to follow: `cfg`-gated module + re-exports.

## Conventions (verified from code)

- **Module pattern** (`process/mod.rs`): `#[cfg(unix)] mod unix; #[cfg(windows)] mod windows;`
  then `#[cfg(unix)] pub(super) use unix::*;` etc. Free functions, no traits.
- **Visibility**: `pub(super)` for intra-`lsp` use, `pub(in crate::lsp)` for
  functions used across `lsp` submodules.
- **Error handling**: `anyhow::Result` with `.context()` at each step.
- **SAFETY comments**: Every `unsafe` block has a `// SAFETY:` comment.
- **File names**: `lsp.req`, `lsp.resp`, `lsp.lock`, `lsp.pid`, `lsp.log` —
  the first three are IPC-specific.

## Implementation Steps

### Step 1: Create `src/lsp/ipc/mod.rs` with interface definition

Create the IPC module entry file defining the public interface and cfg re-exports.

**File structure:**

```rust
// src/lsp/ipc/mod.rs

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use unix::{DaemonIpc, cleanup_ipc_files, ready_indicator, send_command};
#[cfg(windows)]
pub(super) use windows::{DaemonIpc, cleanup_ipc_files, ready_indicator, send_command};

// Re-export poll_retry for daemon.rs ra-stdout polling
// (Unix-only, will be replaced in Phase 3 with transport abstraction)
#[cfg(unix)]
pub(super) use unix::poll_retry;
```

**Public interface (implemented in each platform file):**

```rust
// Opaque struct — fields are platform-specific
pub(in crate::lsp) struct DaemonIpc { .. }

impl DaemonIpc {
    /// Create IPC endpoints (FIFOs on Unix, file dirs on Windows).
    /// Called AFTER ra init — endpoint creation IS the readiness signal.
    /// Cleans up stale endpoint files before creating new ones.
    pub fn setup(daemon_dir: &Path) -> Result<Self>;

    /// Poll for an incoming client request. Returns None on timeout (ms).
    pub fn poll_request(&mut self, timeout_ms: i32) -> Result<Option<DaemonRequest>>;

    /// Send a response to the client.
    pub fn send_response(&mut self, response: &DaemonResponse) -> Result<()>;
}

/// Send a command to the daemon. Acquires exclusive lock, sends request,
/// waits for response with timeout.
pub(in crate::lsp) fn send_command(dir: &Path, req: DaemonRequest, timeout: Duration) -> Result<DaemonResponse>;

/// Remove IPC-specific files (req, resp, lock). Does NOT remove pid/log.
pub(in crate::lsp) fn cleanup_ipc_files(dir: &Path);

/// Path whose existence signals daemon readiness. Clients poll this.
pub(in crate::lsp) fn ready_indicator(dir: &Path) -> PathBuf;
```

- Delegation: main agent
- Depends on: nothing

### Step 2: Create `src/lsp/ipc/unix.rs` — extract from client.rs/daemon.rs

Move the following from `client.rs`:
- `create_fifo()` — make `pub(super)` (used only within ipc/)
- `flock_exclusive()` — make private or `pub(super)`
- `poll_retry()` — re-exported from mod.rs for daemon.rs
- `set_nonblocking()` — re-exported from mod.rs for daemon.rs

Implement the `DaemonIpc` struct for Unix:
```rust
pub(in crate::lsp) struct DaemonIpc {
    req_fd: File,   // lsp.req FIFO, opened O_RDWR + O_NONBLOCK
    resp_fd: File,  // lsp.resp FIFO, opened O_RDWR (blocking)
}
```

- `setup()`: removes stale FIFOs, creates new ones (mode 0o600), opens with
  `O_RDWR` + flags matching current daemon.rs L454-465. Creates lock file.
- `poll_request()`: polls `req_fd` with `poll_retry()`, toggles nonblock,
  reads via `protocol::read_message()`, re-enables nonblock. Returns `None`
  on timeout.
- `send_response()`: writes via `protocol::write_message()` on `resp_fd`.

Implement `send_command()` for Unix:
- Direct extraction of current `client.rs::send_command()` (L140-214).
- Uses `flock_exclusive`, `poll_retry`, `set_nonblocking`, `protocol::*`.

Implement `cleanup_ipc_files()`:
- Removes `lsp.req`, `lsp.resp`, `lsp.lock`.

Implement `ready_indicator()`:
- Returns `dir.join("lsp.req")` (FIFO existence = ready).

- Delegation: main agent
- Depends on: Step 1

### Step 3: Create `src/lsp/ipc/windows.rs` — atomic-rename protocol

Implement the same interface using atomic-rename file protocol per ticket design.

**DaemonIpc:**
```rust
pub(in crate::lsp) struct DaemonIpc {
    daemon_dir: PathBuf,
    // Windows uses file-based polling — no persistent handles
}
```

- `setup()`: creates `lsp.lock` file. Creates a `lsp.ready` marker file
  (the readiness indicator).
- `poll_request()`: checks `lsp.req` existence, reads + deletes on find.
  Uses `std::thread::sleep` for polling (no pipe-based mechanism).
- `send_response()`: writes `lsp.resp.tmp` then renames to `lsp.resp`.

**`send_command()`:**
1. Open `lsp.lock`, acquire exclusive lock via `LockFileEx` (from `windows-sys`).
2. Write `lsp.req.tmp`, rename to `lsp.req`.
3. Poll for `lsp.resp` existence (with timeout).
4. Read + delete `lsp.resp`.
5. Lock released on drop.

**`cleanup_ipc_files()`:** removes `lsp.req`, `lsp.resp`, `lsp.lock`,
`lsp.req.tmp`, `lsp.resp.tmp`, `lsp.ready`.

**`ready_indicator()`:** returns `dir.join("lsp.ready")`.

**Cargo.toml update:** Add `Win32_Storage_FileSystem` and `Win32_System_IO`
to `windows-sys` features (needed for `LockFileEx` and `OVERLAPPED`).

- Delegation: main agent
- Depends on: Step 1

### Step 4: Refactor `client.rs` — remove extracted code

Remove from `client.rs`:
- `create_fifo()`, `flock_exclusive()`, `poll_retry()`, `set_nonblocking()`
- `send_command()` — replaced by `ipc::send_command`
- `cleanup_daemon_files()` — update to call `ipc::cleanup_ipc_files()` plus
  remove non-IPC files (`lsp.pid`, `lsp.log`)

Keep in `client.rs`:
- `daemon_dir()`, `short_hash()` (not IPC-specific)
- `ensure_daemon()`, `spawn_daemon()`, `wait_for_daemon()`, `read_log_tail()`

Update `ensure_daemon()`:
- Change readiness check from `req_fifo.exists()` to
  `ipc::ready_indicator(&dir).exists()`.

Update `wait_for_daemon()`:
- Same: check `ipc::ready_indicator(&dir).exists()`.

Move tests:
- `create_fifo_*`, `flock_*`, `set_nonblocking_*` tests → `ipc/unix.rs`
  (under `#[cfg(test)]`)
- `send_command`-related tests → `ipc/unix.rs` if any (currently none)
- Keep hash/log tests in `client.rs`

- Delegation: main agent
- Depends on: Steps 2, 3

### Step 5: Refactor `daemon.rs` — use `DaemonIpc`

Replace direct FIFO management with `DaemonIpc`:

**Before (lines 444-465, including stale cleanup at L446-447):**
```rust
std::fs::remove_file(&req_path).ok();   // stale cleanup
std::fs::remove_file(&resp_path).ok();  // stale cleanup
create_fifo(&req_path, 0o600)?;
create_fifo(&resp_path, 0o600)?;
File::create(&lock_path)?;
let req_fd = OpenOptions::new()...open(&req_path)?;
let mut resp_fd = OpenOptions::new()...open(&resp_path)?;
```

Note: `DaemonIpc::setup()` must absorb ALL of this — stale cleanup, creation,
and opening with platform-appropriate flags.

**After:**
```rust
let mut ipc = ipc::DaemonIpc::setup(daemon_dir)?;
```

**Main loop — before (L487-507):**
```rust
let mut pfd = libc::pollfd { fd: req_fd.as_raw_fd(), ... };
let n = poll_retry(&mut pfd, 100)?;
if n > 0 { set_nonblocking(&req_fd, false)?; ... }
```

**After:**
```rust
if let Some(request) = ipc.poll_request(100)? {
    // handle request...
    ipc.send_response(&response)?;
}
```

**Cleanup (L612-618):** Replace explicit file removal with `ipc::cleanup_ipc_files()`
plus separate removal of `lsp.pid` and `lsp.log`.

Update imports:
- Remove: `use super::client::{create_fifo, poll_retry, set_nonblocking};`
- Add: `use super::ipc;`
- Keep `poll_retry` import for ra stdout polling: `use super::ipc::poll_retry;`

- Delegation: main agent
- Depends on: Step 4

### Step 6: Update `mod.rs` imports

- Add `mod ipc;` to module list.
- Change `use client::send_command;` → `use ipc::send_command;`.
- `cleanup_daemon_files` and `daemon_dir` stay imported from `client`.
- Update `cmd_stop` (L112) which checks `dir.join("lsp.req").exists()` →
  `ipc::ready_indicator(&dir).exists()`.
- Update `cmd_status` (L164) — same hardcoded `dir.join("lsp.req").exists()`
  check → `ipc::ready_indicator(&dir).exists()`.

- Delegation: main agent
- Depends on: Step 5

### Step 7: Fix `protocol.rs` tests

The protocol tests use `UnixStream::pair()` which is Unix-only. Two options:
- Gate tests with `#[cfg(unix)]` (simpler, these test serialization not IPC)
- Replace with `std::io::Cursor` or pipe (more portable)

Use `std::io::Cursor` for read side and `Vec<u8>` for write side — protocol
is pure byte IO with no platform dependency. This makes the tests portable
without adding cfg gates.

- Delegation: sonnet subagent
- Depends on: nothing (can run in parallel)

### Step 8: Verify compilation

1. `cargo test` — all existing tests pass on macOS.
2. `cargo check --target x86_64-pc-windows-msvc` — IPC module compiles.

- Delegation: main agent
- Depends on: Steps 6, 7

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| `ipc/unix.rs` | Post-impl (migrate existing tests) | Tests for `create_fifo`, `flock_exclusive`, `set_nonblocking` already exist in `client.rs`. Move them. `send_command` and `DaemonIpc` are hard to unit-test (need actual FIFOs + daemon) — covered by integration tests. |
| `ipc/windows.rs` | Manual (cross-compile check) | No Windows CI available. `cargo check --target x86_64-pc-windows-msvc` verifies compilation. Runtime testing deferred to `260326-feat-lsp-windows-support`. |
| `ipc/mod.rs` | N/A | Pure re-exports, no logic to test. |
| `client.rs` | Post-impl (existing integration tests) | `ensure_daemon` / `daemon_dir` logic unchanged. 224 integration tests cover the full LSP pipeline. |
| `daemon.rs` | Post-impl (existing integration tests) | Main loop restructured but behavior identical. Verified by existing `cargo test`. |
| `protocol.rs` | Post-impl (fix existing tests) | Replace `UnixStream::pair()` with `Cursor`/`Vec<u8>` for portability. |

**Key verification scenarios:**
- `cargo test` passes (all 224+ tests, including LSP tests if applicable)
- `cargo check --target x86_64-pc-windows-msvc` passes
- `cargo clippy` clean

## Success Criteria

1. All IPC-specific code (`create_fifo`, `flock_exclusive`, `poll_retry`,
   `set_nonblocking`, `send_command`, FIFO creation/opening in daemon) lives
   in `src/lsp/ipc/` — not in `client.rs` or `daemon.rs`.
2. `client.rs` contains only high-level client logic (daemon lifecycle,
   not raw IPC mechanics).
3. `daemon.rs` main loop uses `DaemonIpc` methods — no direct FIFO/poll/fcntl calls
   (except `poll_retry` for ra stdout, explicitly scoped out).
4. `cargo test` passes with no behavioral changes.
5. `cargo check --target x86_64-pc-windows-msvc` passes.
6. Windows `ipc/windows.rs` implements the atomic-rename protocol per ticket design.
