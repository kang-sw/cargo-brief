# LSP Daemon: FIFO-Based IPC Refactor

## Context
- **Ticket**: `260327-refactor-lsp-fifo-ipc` — replace UDS with FIFO pair + flock
  serialization so all LSP commands work inside Claude Code's macOS sandbox.
- **Problem**: The sandbox blocks all socket syscalls (`socket()`, `bind()`, `connect()`)
  regardless of transport. Named pipes (`mkfifo()`) and `flock()` are allowed.
- **Scope**: All 3 ticket phases in a single plan. Phase 1 is a mechanical refactor
  (generic IO), Phase 2 is the core FIFO replacement, Phase 3 is cleanup.
- **Rejected alternatives**:
  - TCP loopback — also blocked by sandbox (`connect()` syscall)
  - Shared memory — complex, no natural request/response framing
  - stdin/stdout pass-through — doesn't support daemon persistence model
- **Key design decision**: FIFO pair (`lsp.req` + `lsp.resp`) with `flock` serialization.
  Sequential access is acceptable — individual queries complete in <100ms once ra
  is initialized, and the current UDS design is effectively sequential too.
- **Daemon keeps both FIFOs open with O_RDWR for its lifetime.** This eliminates
  POLLHUP races — the daemon is always a reader of `lsp.req` and a writer of
  `lsp.resp`, so clients never see EOF/POLLHUP from a missing counterpart.
  Length-prefix framing handles message boundaries (not fd close).
- **Readiness invariant preserved**: FIFOs are created AFTER ra initialization
  (same position as current UDS bind). `wait_for_daemon()` polls for FIFO
  existence + PID liveness, mirroring current socket-appears detection.
- **PID reuse risk accepted**: Replacing UDS ping with `kill(pid, 0)` loses
  daemon identity proof. PID reuse between daemon death and client check is
  astronomically unlikely (modern 32768+ PID space, ~ms timing window).
  If it occurs, client times out on FIFO operation — not silent corruption.

## Relevant Files
- `src/lsp/protocol.rs` — message framing functions (`write_message`, `read_message`);
  `DaemonRequest`/`DaemonResponse` enums (unchanged). **Phase 1 target.**
- `src/lsp/client.rs` — `ensure_daemon()`, `send_command()`, `spawn_daemon()`,
  `try_connect()`, `wait_for_socket()`, `daemon_dir()`. **Phase 2 primary target.**
- `src/lsp/daemon.rs` — `run_daemon()`, `run_daemon_from_args()`, `handle_client()`.
  **Phase 2 primary target.**
- `src/lsp/mod.rs` — `cmd_touch()`, `cmd_stop()`, `cmd_query()`, `cmd_status()`.
  All use `UnixStream::connect()` + `send_command()`. **Phase 2 callers.**
- `src/lsp/transport.rs` — ra stdio communication. **Unchanged.**
- `src/lsp/watcher.rs` — FS event watcher. **Unchanged.**
- `src/lsp/query.rs` — query handlers. **Unchanged.**
- `src/main.rs` — `__lsp-daemon` early-exit (line 9). **Unchanged.**
- `ai-docs/mental-model/lsp-daemon.md` — mental model doc. **Phase 3 update.**

## Conventions (verified from code)
- `libc` is already a dependency (used in `client.rs::process_alive()` for `kill(pid, 0)`)
- All LSP code is `#[cfg(unix)]` gated
- Daemon dir: `<target_dir>/cargo-brief-lsp/<hash>/` (FNV-1a of canonical workspace root)
- Current daemon files: `lsp.pid`, `lsp.sock`, `lsp.log`
- New daemon files: `lsp.pid`, `lsp.req` (FIFO), `lsp.resp` (FIFO), `lsp.lock`, `lsp.log`
- Error handling: `anyhow::Result` + `.context()` at each step
- Daemon args parsed manually in `run_daemon_from_args()` (no clap)
- `protocol.rs` already imports `std::io::{Read, Write}` but uses `&mut UnixStream` in sigs
- Existing roundtrip tests use `UnixStream::pair()` — these are test-only, keep working
- `send_command` is used by `mod.rs` commands after `UnixStream::connect()`
- `handle_client` in daemon.rs takes owned `UnixStream` from `listener.accept()`

## Implementation Steps

### Phase 1: Abstract protocol over generic Read/Write

1. **Genericize `write_message` and `read_message` in `protocol.rs`**
   - `write_message(stream: &mut UnixStream, ...)` → `write_message(writer: &mut impl Write, ...)`
   - `read_message<T>(stream: &mut UnixStream)` → `read_message<T>(reader: &mut impl Read)`
   - Remove `use std::os::unix::net::UnixStream` from protocol.rs (non-test code)
   - Update doc comments: "UDS stream" → "stream" (generic)
   - All callers pass `&mut stream` where `stream: UnixStream` — trait bounds satisfied
     automatically, no caller changes needed.
   - Delegation: main
   - **Checkpoint**: `cargo test` — all protocol roundtrip tests pass unchanged
     (test code still uses `UnixStream::pair()`, which impls `Read + Write`)
   - **Commit** after this step (Phase 1 complete)

### Phase 2: Replace UDS with FIFO + flock

2. **Add FIFO/flock helper functions in `client.rs`**
   - `fn create_fifo(path: &Path, mode: u32) -> Result<()>` — wraps `libc::mkfifo()`
     with `CString` conversion and errno handling. Ignores `EEXIST` (idempotent).
   - `fn flock_exclusive(file: &File) -> Result<()>` — wraps `libc::flock(fd, LOCK_EX)`
   - `fn set_nonblocking(file: &File, nonblock: bool) -> Result<()>` — wraps
     `libc::fcntl(F_GETFL/F_SETFL)` to toggle `O_NONBLOCK`
   - These are thin wrappers over `libc` calls; `libc` is already a dep.
   - Uses: `std::os::unix::io::AsRawFd`, `std::ffi::CString`, `std::os::unix::ffi::OsStrExt`
   - Delegation: main

3. **Restructure daemon directory layout**
   - `daemon_dir()` unchanged (returns `<target>/cargo-brief-lsp/<hash>/`)
   - New file constants (or inline): `"lsp.req"`, `"lsp.resp"`, `"lsp.lock"`, `"lsp.pid"`, `"lsp.log"`
   - Remove all references to `"lsp.sock"` across the codebase:
     - `client.rs`: `ensure_daemon` (line 31), `try_connect` (called from ensure_daemon)
     - `mod.rs`: `cmd_touch` (line 86), `cmd_stop` (line 114), `cmd_query` (line 150),
       `cmd_status` (line 169)
     - `daemon.rs`: `run_daemon` (lines 200–357, references socket_path throughout:
       stale cleanup, UnixListener::bind, eprintln, cleanup remove_file, parent dir)
   - Delegation: main

4. **Replace daemon main loop in `daemon.rs`**
   - `run_daemon_from_args()`: replace `--socket` + `--pid-file` with `--daemon-dir`.
     Derive all paths: `dir.join("lsp.pid")`, `dir.join("lsp.req")`, `dir.join("lsp.resp")`,
     `dir.join("lsp.lock")`.
   - `run_daemon()` signature: `(workspace_root, daemon_dir)` instead of
     `(workspace_root, socket_path, pid_path)`.
   - Startup sequence (preserving readiness invariant — FIFOs created AFTER ra init):
     1. Write PID file (early, prevents double-spawn)
     2. Discover ra, spawn, initialize LSP (unchanged)
     3. Start file watcher (unchanged)
     4. Clean stale FIFOs (remove + recreate): `remove_file` then `create_fifo()`
        for `lsp.req`, `lsp.resp`. Create `lsp.lock` with `File::create()`.
     5. Open `lsp.req` with `O_RDWR | O_NONBLOCK` — daemon keeps open for lifetime.
        `O_RDWR` prevents `open()` from blocking; `O_NONBLOCK` makes `read()` non-blocking.
     6. Open `lsp.resp` with `O_RDWR` — daemon keeps open for lifetime.
        `O_RDWR` ensures the write-end always exists; clients never see POLLHUP.
   - Main loop replaces `listener.accept()` with `poll()`-based FIFO polling:
     ```
     // req_fd: opened O_RDWR | O_NONBLOCK (kept open for daemon lifetime)
     // resp_fd: opened O_RDWR (kept open for daemon lifetime)
     loop {
         // Poll lsp.req for incoming data (POLLIN)
         let mut pfd = pollfd { fd: req_fd.as_raw_fd(), events: POLLIN, revents: 0 };
         let n = poll(&mut pfd, 1, 100 /* ms */);
         if n > 0 && (pfd.revents & POLLIN) != 0 {
             // Client sent data — switch to blocking, read full message
             set_nonblocking(&req_fd, false)?;
             let request: DaemonRequest = read_message(&mut req_fd)?;
             set_nonblocking(&req_fd, true)?;
             // Process (pure function, no IO)
             let response = handle_request(&request, ...);
             // Write response to lsp.resp (already open O_RDWR, just write)
             write_message(&mut resp_fd, &response)?;
             last_activity = Instant::now();
         } else {
             // No client or timeout — check idle
             if last_activity.elapsed() > idle_timeout { break; }
         }
         // Drain FS events (unchanged)
         // Check ra alive (unchanged)
     }
     ```
   - Response writing: since `lsp.resp` is kept open with `O_RDWR` by the daemon,
     just call `write_message(&mut resp_fd, &response)`. The client reads from its
     own O_RDONLY fd. Length-prefix framing handles message boundaries.
   - `handle_client()` → rename to `handle_request()`, change signature: takes
     `&DaemonRequest` and returns `DaemonResponse` (pure function, no IO).
     Query variants still take `&mut RaTransport` and `&Path workspace_root`.
   - Cleanup: remove FIFO files, PID file, lock file, log file on exit.
   - Delegation: main
   - Depends on: steps 1, 2, 3

5. **Replace client connection in `client.rs`**
   - `ensure_daemon()`: change return type from `Result<UnixStream>` to `Result<PathBuf>`
     (returns daemon dir path). Liveness check: read `lsp.pid`, check `process_alive(pid)`,
     AND verify `lsp.req` exists (FIFO existence = daemon ready, same invariant as
     current socket-appears check). If alive + FIFOs exist, return dir. If dead or
     FIFOs missing, clean up and respawn.
   - Remove `try_connect()` (replaced by PID + FIFO existence check).
   - `send_command()`: new signature `send_command(daemon_dir: &Path, request: DaemonRequest,
     timeout: Duration) -> Result<DaemonResponse>`:
     1. Open `lsp.lock` with `O_CREAT | O_RDWR` (idempotent — handles case where
        daemon hasn't created it yet, e.g., `cmd_stop` without `ensure_daemon`).
        Call `flock_exclusive()`.
     2. Open `lsp.req` with `O_WRONLY` (blocks until daemon has R end — instant since
        daemon keeps it open with O_RDWR)
     3. `write_message(&mut req_fd, &request)`, drop `req_fd`
     4. Open `lsp.resp` with `O_RDONLY | O_NONBLOCK`
     5. `libc::poll()` with `timeout` millis for `POLLIN` — since daemon keeps
        `lsp.resp` open with `O_RDWR`, the write-end always exists, so `poll()`
        returns `POLLIN` when data arrives (never `POLLHUP` spuriously).
     6. `set_nonblocking(resp_fd, false)`, `read_message(&mut resp_fd)`
     7. flock auto-released on `lock_fd` drop
   - `wait_for_socket()` → rename to `wait_for_daemon()`: poll for `lsp.req` FIFO
     existence + `process_alive()` with exponential backoff (50ms → 500ms).
     Returns `Result<PathBuf>` (daemon dir). Still checks `child.try_wait()` for
     early death detection + log tail on failure.
   - `spawn_daemon()`: change args from `["__lsp-daemon", "--workspace-root", ws,
     "--socket", sock, "--pid-file", pid]` to `["__lsp-daemon", "--workspace-root",
     ws, "--daemon-dir", dir]`.
   - Delegation: main
   - Depends on: steps 2, 3, 4

6. **Update mod.rs command functions**
   - `cmd_touch()`: call `ensure_daemon()` (returns dir), call
     `send_command(&dir, DaemonRequest::Status, 5s)`.
   - `cmd_stop()`: call `daemon_dir()` to get dir (NOT `ensure_daemon` — stop
     shouldn't spawn), try `send_command(&dir, DaemonRequest::Stop, 5s)`, then
     clean up files. If FIFOs don't exist, assume daemon not running.
   - `cmd_query()`: call `ensure_daemon()` (returns dir), call
     `send_command(&dir, request, 30s)`.
   - `cmd_status()`: call `daemon_dir()` to get dir (NOT `ensure_daemon`), try
     `send_command(&dir, DaemonRequest::Status, 5s)`, print result. If FIFOs don't
     exist or send fails, print "not running".
   - Remove all `std::os::unix::net::UnixStream` usage from mod.rs.
   - Delegation: main
   - Depends on: step 5

7. **Verify and commit Phase 2**
   - Run `cargo test` — all existing protocol roundtrip tests pass.
   - Run `cargo clippy` — no warnings.
   - Manual smoke test: `cargo brief lsp touch`, `cargo brief lsp status`,
     `cargo brief lsp stop` in a Rust project.
   - If possible: test inside Claude Code sandbox (no socket permission prompts).
   - Verify: idle timeout triggers shutdown (set `CARGO_BRIEF_LSP_TIMEOUT=5`).
   - Verify: stale FIFO cleanup on daemon restart (kill -9 daemon, then touch).
   - **Commit** after this step (Phase 2 complete)

### Phase 3: Cleanup

8. **Remove dead UDS code and verify**
   - `grep -r "UnixStream\|UnixListener\|unix::net" src/lsp/` — should find nothing
     except in `protocol.rs` test code (which uses `UnixStream::pair()` for testing).
   - Protocol test code can keep `UnixStream::pair()` since it tests the generic
     Read/Write impl. Alternatively, replace with `pipe()` or `Vec<u8>` cursor — optional.
   - Remove `use std::os::unix::net::UnixStream` from `client.rs`, `daemon.rs`, `mod.rs`.
   - Update `protocol.rs` module doc: "UDS message types" → "message types".
   - Update `daemon.rs` module doc: "accepts UDS clients" → "accepts FIFO clients".
   - Update `mod.rs` module doc: "via Unix domain socket" → "via named pipes".
   - Delegation: main

9. **Update mental model and docs**
   - Update `ai-docs/mental-model/lsp-daemon.md`:
     - Entry points: update `client.rs` function signatures
     - Module contracts: update `daemon_dir` (new file layout), `ensure_daemon`
       (PID-based, returns PathBuf), `run_daemon` (FIFO poll loop), `handle_client`
       → `handle_request` (pure function), `send_command` (flock + FIFO)
     - Remove socket-related coupling notes, add FIFO-related ones
     - Update extension points
     - Update common mistakes section
     - Remove UDS-related tech debt, add any new FIFO-related debt
   - Update `ai-docs/_index.md`: `lsp/` description — "UDS framing" → "FIFO IPC"
   - Append `### Result` to the ticket after all phases complete.
   - Delegation: main (mental-model-updater subagent for lsp-daemon.md)
   - **Commit** after this step (Phase 3 complete)

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| `protocol.rs` | Existing tests (post-impl verify) | Phase 1 is a signature change; existing `UnixStream::pair()` roundtrip tests validate generic impls |
| `client.rs` helpers | Post-impl unit tests | `create_fifo`, `flock_exclusive`, `set_nonblocking` are thin libc wrappers; test in tmpdir |
| `client.rs` send_command | Manual test | Requires running daemon; smoke-test via `cargo brief lsp touch/status/stop` |
| `daemon.rs` FIFO loop | Manual test | Integration-level; smoke-test with `CARGO_BRIEF_LSP_TIMEOUT=5` for idle shutdown |
| `mod.rs` commands | Manual test | End-to-end via `cargo brief lsp {touch,status,stop,references}` |
| Sandbox compat | Manual test | Run inside Claude Code sandbox; verify no permission prompts |

**Post-impl unit tests for new helpers (step 2):**
- `create_fifo`: creates FIFO in tmpdir, verify with `std::fs::metadata().file_type().is_fifo()` (unix ext)
- `create_fifo` idempotent: calling twice on same path succeeds
- `flock_exclusive`: acquire lock, verify second attempt in same thread blocks (or use LOCK_NB to verify EWOULDBLOCK)
- `set_nonblocking`: toggle on a file, verify flag state via fcntl

**Protocol roundtrip tests (verify unchanged):**
- All 6 existing tests in `protocol.rs::tests` must pass unchanged after Phase 1.
  They use `UnixStream::pair()` which implements `Read + Write`.

## Success Criteria
- `cargo test` passes after each phase commit
- `cargo clippy` clean after each phase commit
- `grep -r "socket\|UnixStream\|UnixListener" src/lsp/` shows no socket usage
  outside of test code (Phase 3)
- `cargo brief lsp touch/stop/status/references` work end-to-end
- All operations work inside Claude Code macOS sandbox without permission prompts
- Idle timeout (`CARGO_BRIEF_LSP_TIMEOUT=5`) triggers clean daemon shutdown
- Stale FIFO cleanup: `kill -9 <daemon-pid>` then `cargo brief lsp touch` succeeds
- `DaemonRequest::Ping` variant can be removed (no longer needed — PID liveness
  replaces ping) — optional cleanup
