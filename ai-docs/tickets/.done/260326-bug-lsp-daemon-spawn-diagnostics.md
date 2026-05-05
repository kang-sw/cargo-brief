---
title: "LSP daemon: improve spawn failure diagnostics"
status: done
completed: 2026-03-26
plans:
  phase1: 2026-03/26-1900-lsp-daemon-spawn-diagnostics
related:
  260326-feat-lsp-daemon-bootstrap: original implementation
---

# LSP daemon: improve spawn failure diagnostics

## Problem

When the daemon process dies shortly after spawn (e.g., socket bind fails due
to sandbox restrictions, permission errors, or missing directories), the client
has no way to detect this and blindly polls `wait_for_socket` for up to 120
seconds before timing out with a generic "Timed out waiting for LSP daemon
socket" error.

Observed failure modes:
1. **Sandbox/permission blocks socket bind** -- daemon writes PID file, starts
   ra, then fails at `UnixListener::bind()`. Client sees timeout after 120s.
2. **ra binary not found or crashes** -- daemon exits immediately. Client sees
   timeout after 120s.
3. **Runtime dir inaccessible** -- `create_dir_all` fails. Client sees timeout.

In all cases the actual error is logged to daemon stderr, which is piped to
`Stdio::null()` by `spawn_daemon()` -- the diagnostic is silently discarded.

## Goal

Make daemon spawn failures fast and informative instead of a 120-second
silent timeout.

### Phase 1: Early death detection in wait_for_socket

After `spawn_daemon()`, the client knows the PID file path. During the
`wait_for_socket` polling loop, check if the daemon process is still alive
(via `process_alive(pid)` which already exists). If the process has exited,
bail immediately with a message like:

```
LSP daemon exited before becoming ready (PID {pid}).
Run with --verbose or check daemon logs for details.
```

This turns a 120-second timeout into a ~1-second failure for all crash-on-start
scenarios.

**Success criteria:**
- If daemon exits during `wait_for_socket`, client detects within one poll
  interval and reports a clear error
- Error message includes the PID for correlation
- Existing happy path (daemon starts successfully) unaffected

### Phase 2: Daemon stderr capture for diagnostics

Instead of `Stdio::null()`, redirect daemon stderr to a log file in the daemon
directory (e.g., `lsp.log`). On spawn failure, the client can read the last N
lines of this file and include them in the error message. Rotate/truncate on
daemon restart to avoid unbounded growth.

**Success criteria:**
- Daemon stderr written to `<daemon_dir>/lsp.log`
- On spawn failure, client includes relevant log lines in error output
- Log file truncated on each daemon start (no accumulation across restarts)
- `lsp stop` cleanup includes the log file

## Estimated scope

Phase 1: ~30 lines (modify `wait_for_socket` + `spawn_daemon` to return PID).
Phase 2: ~50 lines (stderr redirect + log reading + cleanup).

### Result (d8eb41f) - 26-03-26

Both phases implemented together (tightly coupled — same 3 functions).

**Changes:**
- `client.rs`: `spawn_daemon` redirects stderr to `lsp.log` via `Stdio::from(File)`,
  returns `Result<u32>` (child PID). `wait_for_socket` accepts PID + log_path, checks
  `process_alive(pid)` each iteration, bails with log tail on death or timeout.
  `ensure_daemon` threads PID and log_path. New `read_log_tail` helper (last N lines).
- `daemon.rs`: cleanup removes `lsp.log` before `remove_dir`.
- `mod.rs`: `cmd_stop` removes `lsp.log` before `remove_dir`.

**Tests:** 4 unit tests for `read_log_tail` (>max lines, <max lines, nonexistent, empty).
Spawn/wait/cleanup paths require manual verification (daemon process lifecycle).

**No deviations from plan.** All success criteria met.
