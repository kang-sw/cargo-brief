---
title: "LSP daemon: process lifecycle, UDS, ra bootstrap"
---

# LSP daemon: process lifecycle, UDS, ra bootstrap

**Parent:** `260326-feat-lsp-daemon`

## Goal

Implement the daemon infrastructure: spawn/connect lifecycle, UDS
server/client, PID management, idle timeout, and rust-analyzer LSP
bootstrap (initialize → ready → shutdown).

## Deliverables

### CLI surface

Add `LspArgs` / `LspCommand` to `cli.rs`:

```rust
pub enum BriefCommand {
    // ... existing variants ...
    /// Manage LSP daemon for semantic code analysis
    Lsp(LspArgs),
}

pub struct LspArgs {
    pub command: LspCommand,
}

pub enum LspCommand {
    Touch,
    Stop,
    Status,
}
```

### Daemon process (`src/lsp/daemon.rs`)

- Spawned as a background child process (double-fork or `daemonize` pattern)
- Event loop: UDS accept + idle timeout
- PID file at `$XDG_RUNTIME_DIR/cargo-brief/{workspace_hash}/lsp.pid`
- UDS socket at `.../{workspace_hash}/lsp.sock`
- Workspace hash: SHA-256 of canonical workspace root path, truncated
- Idle timeout: 10 minutes default, reset on each client interaction
- Signal handling: SIGTERM → graceful shutdown

### Client connect (`src/lsp/client.rs`)

- `ensure_daemon(workspace_root) -> Result<UdsStream>`
  1. Check PID file → process alive?
  2. If alive, try UDS connect + health ping
  3. If not → spawn daemon, wait for ready signal (UDS becomes connectable)
  4. Return connected stream
- Timeout on daemon startup (e.g., 120s for large projects)
- Progress output on stderr during ra warm-up

### LSP bootstrap

- Spawn `rust-analyzer` subprocess with stdio transport
- Send `initialize` request with workspace root
- Wait for `initialized` confirmation
- Handle ra crash: detect broken pipe, report to client, cleanup

### Module structure

```
src/lsp/
  mod.rs        — public interface, re-exports
  daemon.rs     — daemon process, event loop, lifecycle
  client.rs     — client-side connect/spawn logic
  protocol.rs   — UDS message framing (length-prefixed JSON or similar)
  transport.rs  — LSP JSON-RPC over stdio to ra
```

## Acceptance criteria

- `cargo brief lsp touch` starts daemon, prints "LSP daemon ready (pid NNN)"
- `cargo brief lsp status` shows daemon state + ra indexing status
- `cargo brief lsp stop` gracefully terminates daemon + ra
- Second `touch` is a no-op (daemon already running)
- After idle timeout, daemon exits and cleans up PID/socket files
- Daemon crash → next command auto-restarts

## Estimated scope

~1000-1200 lines
