---
title: "LSP daemon: replace UDS with FIFO-based IPC for sandbox compatibility"
status: done
started: 2026-03-27
completed: 2026-03-27
plans:
  phase1: 2026-03/27-0138-lsp-fifo-ipc
related:
  260326-feat-lsp-daemon-bootstrap: original UDS implementation
  260326-feat-lsp-query-commands: query commands using current IPC
  260326-feat-lsp-windows-support: shares IPC abstraction concern
---

# LSP daemon: FIFO-based IPC

## Problem

Claude Code's macOS sandbox blocks all socket syscalls (`socket()`, `bind()`,
`connect()`) regardless of transport (UDS, TCP, UDP) or path. This makes every
`cargo brief lsp` command fail inside the sandbox, requiring the user to approve
sandbox bypass on each invocation.

Verified experimentally:
- UDS `bind()` in `target/` — blocked
- TCP `bind("127.0.0.1", 0)` — blocked
- TCP `connect()` to localhost — blocked
- `mkfifo()` in `target/` — **allowed**
- `flock()` on regular file — **allowed**
- Regular file read/write — **allowed**

## Goal

Replace the UDS client-daemon IPC with named pipes (FIFO) + `flock` serialization
so that all `cargo brief lsp` commands work inside sandboxed environments without
any permission prompts.

## Design

### IPC model: FIFO pair + flock serialization

Two named pipes for bidirectional communication, with a file lock ensuring
one client at a time:

```
target/cargo-brief-lsp/<hash>/
  lsp.pid       — daemon PID
  lsp.lock      — flock (client serialization)
  lsp.req       — FIFO: client writes, daemon reads
  lsp.resp      — FIFO: daemon writes, client reads
  lsp.log       — daemon stderr
```

### Protocol sequence

```
Client (under flock)              Daemon (loop)
────────────────────              ──────────────
1. flock(LOCK_EX)
2. open(req, W)  ───────────────  open(req, R)     ← mutual unblock
3. write request ──────────────→  read request
4. close req                      process...
5. open(resp, R) ───────────────  open(resp, W)    ← mutual unblock
6. read response ←──────────────  write response
7. close resp                     close resp → loop
8. flock(LOCK_UN)
```

FIFO `open()` blocks until both ends connect — this provides natural
synchronization without polling. The flock ensures only one client executes
this sequence at a time.

### Framing

The existing 4-byte LE length-prefix + JSON protocol is preserved unchanged.
`protocol.rs` functions change from `&mut UnixStream` to generic
`impl Read` / `impl Write` (or `&mut File`).

### Daemon idle timeout

The daemon cannot block indefinitely on FIFO `open()` (would prevent idle
shutdown). Use `O_RDWR | O_NONBLOCK` to open the request FIFO, then poll
with 100ms sleep intervals — same pattern as the current `UnixListener`
non-blocking accept loop. When no client connects within the timeout
period, the daemon shuts down.

### Sequential access is acceptable

Individual ra queries complete in <100ms once initialized. Even under
concurrent usage (unlikely for a dev tool), the flock serialization adds
negligible latency. The current UDS design is also effectively sequential
(single-threaded daemon with blocking `handle_client`).

### Ping/liveness check

Current approach: `try_connect()` opens UDS, sends Ping, reads Pong.
New approach: check PID file + `kill(pid, 0)` for liveness (already
implemented as `process_alive()`). The FIFO `open()` blocking behavior
makes a lightweight ping impractical — use PID-based liveness instead.

For `cmd_touch`, after `ensure_daemon()` confirms liveness, acquire flock
and send a Status request through the FIFO pair to report daemon state.

### Impact on `#[cfg(unix)]`

`mkfifo` is Unix-only (`nix::unistd::mkfifo` or `libc::mkfifo`). The
`#[cfg(unix)]` gate remains. Windows support (via named pipes) is tracked
separately in `260326-feat-lsp-windows-support`.

However, abstracting `protocol.rs` to generic `Read`/`Write` is a shared
prerequisite for both this ticket and Windows support.

## Phases

### Phase 1: Abstract protocol over generic Read/Write

Decouple `protocol.rs` from `UnixStream`. Change `write_message` and
`read_message` to accept `impl Write` / `impl Read`. Update all callers.
Existing UDS behavior unchanged — purely a refactor.

**Success criteria:** `cargo test` passes, no functional change.

### Phase 2: Replace UDS with FIFO + flock

Replace `UnixListener`/`UnixStream` in daemon and client with FIFO pair
and flock-based serialization. Daemon loop switches from non-blocking
accept to non-blocking FIFO poll. Client `ensure_daemon` uses PID-based
liveness instead of ping.

**Success criteria:**
- `cargo brief lsp touch/stop/status` work
- `cargo brief lsp references/blast-radius/call-hierarchy` work
- All operations work inside Claude Code sandbox without permission prompts
- Idle timeout still triggers daemon shutdown
- Existing unit tests pass (protocol roundtrip, formatting)
- Stale FIFO cleanup on daemon restart

### Phase 3: Cleanup

Remove dead UDS code, update mental model and docs, verify no socket
syscalls remain in the lsp module.

### Result (527c6e6) - 26-03-27

All three phases implemented in a single session:

**Phase 1** (31036b5): `write_message`/`read_message` genericized to `impl Write`/`impl Read`. All 6 existing roundtrip tests pass unchanged.

**Phase 2** (7ebd956): Full UDS → FIFO replacement:
- `daemon.rs`: `poll()`-based main loop on `lsp.req`, `O_RDWR` keeps FIFOs open for daemon lifetime, `handle_client` → `handle_request` (takes `&DaemonRequest`, returns `DaemonResponse`), `--daemon-dir` replaces `--socket`+`--pid-file`
- `client.rs`: `ensure_daemon` returns `PathBuf`, `send_command` uses `flock` + FIFO write/poll/read with timeout, `wait_for_daemon` polls FIFO existence, new helpers (`create_fifo`, `flock_exclusive`, `set_nonblocking`)
- `mod.rs`: All `cmd_*` use daemon dir + `send_command` with explicit timeouts

**Phase 3** (527c6e6, code review fixes): Removed `DaemonRequest::Ping` (dead after UDS removal). Added `poll_retry()` for EINTR safety. Added stale FIFO data drain. Updated mental model, `_index.md`, `_memory.md`.

**Deviations:**
- Plan's "daemon keeps FIFOs open with O_RDWR" design preserved exactly
- Plan suggested `handle_request` as "pure function, no IO" — review caught this was incorrect (query handlers do IO via transport); doc comment corrected
- Stale FIFO data race (crashed client leaving orphaned response) not in plan — added client-side drain as mitigation during review
- EINTR handling not in plan — added during code review
