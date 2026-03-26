# LSP Daemon Spawn Diagnostics

## Context
- **Ticket:** `260326-bug-lsp-daemon-spawn-diagnostics` (both phases)
- **Problem:** When the daemon process dies shortly after spawn (sandbox blocks
  socket bind, ra not found, permission errors), the client polls for up to
  120 seconds before reporting a generic timeout. The daemon's actual error
  message is piped to `Stdio::null()` and lost.
- **Discovered during:** manual testing of `lsp references` command in a
  sandboxed Claude Code environment.
- **Goal:** Fast failure (~1s) with actionable error messages.

### Key decisions
- **PID-based death detection:** `spawn_daemon` already writes a PID file
  before binding the socket. The client can read this PID and check
  `process_alive()` (already exists) on each polling iteration. No new IPC
  mechanism needed.
- **Log file, not pipe:** Daemon runs in a detached process group
  (`process_group(0)`), so we can't capture its stderr via the spawning
  process's pipe. Redirect stderr to a file in the daemon directory instead.
  The daemon dir already exists by the time `spawn_daemon` is called.
- **Truncate on start:** `File::create` (not append) for the log redirect,
  so each daemon start gets a fresh log. No rotation needed.
- **Tail on failure:** Read last 20 lines of `lsp.log` when death is detected
  or timeout occurs. Include them in the error message.
- **Both phases in one plan:** They touch the same 3 functions and are
  tightly coupled (PID detection alone gives "daemon died" but no _why_;
  log capture alone gives logs but no fast detection). Together they produce
  the intended UX.

## Relevant Files
- `src/lsp/client.rs` — `spawn_daemon()` (line 84): stderr redirect, return PID.
  `wait_for_socket()` (line 116): add PID liveness check. `ensure_daemon()` (line 23):
  thread PID from spawn to wait.
- `src/lsp/mod.rs` — `cmd_stop()` (line 69): add `lsp.log` to cleanup.
- `src/lsp/daemon.rs` — `run_daemon()` (line 166): add `lsp.log` to exit cleanup
  (line 310-315).

## Conventions (verified from code)
- `daemon_dir()` returns the per-workspace directory. Files in it: `lsp.sock`,
  `lsp.pid`. Adding `lsp.log` follows the same pattern.
- Error messages use `bail!()` with multi-line format strings (see `wait_for_socket`).
- `process_alive(pid: u32) -> bool` exists in client.rs (line 138), uses
  `libc::kill(pid, 0)`.
- Daemon cleanup in `run_daemon()` removes `pid_path` and `socket_path` with
  `.ok()` (ignore errors). `cmd_stop()` does the same plus `remove_dir`.

## Implementation Steps

1. **`spawn_daemon` → redirect stderr to `lsp.log`, return PID**
   - Add `--log-file` argument to the daemon command args (passed alongside
     `--socket` and `--pid-file`)
   - Open `daemon_dir/lsp.log` with `File::create` (truncates)
   - Change `.stderr(Stdio::null())` to `.stderr(Stdio::from(log_file))`
   - Change return type from `Result<()>` to `Result<u32>` (child PID via
     `child.id()`)
   - Delegation: main
   - Depends on: none

2. **`run_daemon_from_args` → accept `--log-file` arg (ignored, for future use)**
   - Actually: No. The daemon doesn't need to know about the log file path —
     stderr is already redirected by the parent process via `Stdio::from()`.
     The daemon just writes to stderr normally. No daemon-side change needed
     for this.
   - **Revised:** Skip this step. The daemon writes to stderr as before;
     the parent's `Stdio::from(file)` handles the redirect transparently.

3. **`wait_for_socket` → accept PID, check liveness each iteration**
   - Change signature: `wait_for_socket(sock: &Path, timeout: Duration, pid: u32, log_path: &Path) -> Result<UnixStream>`
   - In the polling loop, after `try_connect` fails and before sleeping,
     call `process_alive(pid)`. If false, read tail of `log_path` and bail
     with a diagnostic message.
   - On timeout (loop exhausted), also read log tail and include in the
     timeout error message.
   - Helper: `fn read_log_tail(path: &Path, max_lines: usize) -> String`
     — reads file, returns last N lines joined with newlines. Returns
     empty string if file doesn't exist or is unreadable.
   - When including log tail in error messages, if the tail is empty, show
     "(no log output)" to avoid a confusing blank section.
   - Delegation: main
   - Depends on: step 1

4. **`ensure_daemon` → thread PID and log_path through**
   - `spawn_daemon` now returns `Result<u32>` (PID)
   - Compute `log_path = dir.join("lsp.log")`
   - Pass PID and `&log_path` to `wait_for_socket`
   - Delegation: main
   - Depends on: steps 1, 3

5. **`run_daemon` → add `lsp.log` to exit cleanup**
   - Remove `socket_path.with_file_name("lsp.log")` with `.ok()`.
   - **Ordering:** Must come before the `remove_dir(parent)` call, otherwise
     the directory removal silently fails (dir not empty).
   - Delegation: main
   - Depends on: none

6. **`cmd_stop` → add `lsp.log` to cleanup**
   - Add `std::fs::remove_file(dir.join("lsp.log")).ok();` after sock/pid
     removal but **before** `remove_dir(&dir)`.

**Note:** Steps 1, 3, 4 change interconnected signatures (`spawn_daemon`
return type, `wait_for_socket` parameters, `ensure_daemon` call site).
They must all be applied before the code compiles. Implement sequentially
in one pass.
   - Delegation: main
   - Depends on: none

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| `read_log_tail` | Post-impl | Pure file I/O, easy to test with temp files |
| `spawn_daemon` return type | Manual | Requires actual binary execution |
| `wait_for_socket` PID check | Manual | Requires daemon process lifecycle |
| Cleanup paths | Manual | Requires running daemon |

### Post-impl module: `read_log_tail`
- **Key scenarios:**
  - File with >20 lines → returns last 20
  - File with <20 lines → returns all lines
  - File doesn't exist → returns empty string
  - Empty file → returns empty string

### Manual verification
1. Build, then trigger a failure: `TMPDIR=/nonexistent cargo brief lsp touch -v`
   — should fail fast with daemon stderr in error message
2. Normal `cargo brief lsp touch` — should work as before
3. `cargo brief lsp stop` — should clean up `lsp.log`
4. Kill daemon manually, then `cargo brief lsp touch` — should detect death
   and show log tail

## Success Criteria
- Daemon early death detected within one poll interval (~50-500ms), not 120s
- Error message includes last lines of daemon stderr (e.g., "Operation not
  permitted", "rust-analyzer not found")
- Normal daemon startup unaffected (happy path identical)
- `lsp stop` and daemon exit both clean up `lsp.log`
- No new compilation warnings
- `cargo test` passes
- `read_log_tail` unit tests pass
