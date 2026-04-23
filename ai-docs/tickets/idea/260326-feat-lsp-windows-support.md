---
title: "LSP daemon: Windows platform support"
status: idea
related:
  260326-feat-lsp-daemon-bootstrap: original Unix-only implementation
  260326-bug-lsp-daemon-spawn-diagnostics: spawn diagnostics (also Unix-only)
---

# LSP daemon: Windows platform support

## Problem

The entire `lsp` module is `#[cfg(unix)]` — it does not compile on Windows.
Key blockers: `std::os::unix::net` (UDS), `libc::kill` (PID check),
`CommandExt::process_group` (daemon detach), `XDG_RUNTIME_DIR` (socket dir).

No Windows test environment is currently available, so this ticket stays in
`idea/` until one is set up.

## Goal

Make `cargo brief lsp {touch,stop,status,references}` work on Windows with
the same semantics as Unix.

## Design Notes

### IPC: Named Pipes vs TCP localhost

- **Named pipes** (`\\.\pipe\cargo-brief-<hash>`): Windows-native, no port
  conflicts, permission model similar to UDS. Recommended.
- **TCP localhost**: cross-platform but exposes a port, needs port allocation,
  firewall concerns.
- The length-prefix framing in `protocol.rs` works over any byte stream —
  only the connection/listen layer needs abstraction.

### IPC abstraction

Introduce a `trait DaemonStream: Read + Write` (or just use `Box<dyn Read + Write>`)
to decouple client/daemon from concrete socket types. Platform-specific modules
provide the connection and listener implementations behind `#[cfg]`.

### Process management

| Unix | Windows equivalent |
|------|--------------------|
| `process_group(0)` | `CREATE_NEW_PROCESS_GROUP` + `DETACHED_PROCESS` creation flags |
| `libc::kill(pid, 0)` | `OpenProcess(SYNCHRONIZE, pid)` + `CloseHandle` |
| `child.try_wait()` | Already cross-platform (`std`) — no change needed |

### Runtime directory

`XDG_RUNTIME_DIR` → `%LOCALAPPDATA%\cargo-brief\lsp\` on Windows. Named pipes
don't need a filesystem directory for the pipe itself, but PID file and log file
still need a location.

### Unaffected components

- `watcher.rs` — `notify` crate is already cross-platform
- `transport.rs` — RA stdio communication uses `std::process::Command`
- `protocol.rs` — pure serialization over byte streams
- `query.rs` — pure LSP JSON-RPC logic

## Phases

### Phase 1: IPC abstraction layer

Extract platform-specific IPC into a module with `#[cfg(unix)]` / `#[cfg(windows)]`
backends. Unix side is a refactor (extract trait, wrap existing UDS code). Windows
side provides named pipe implementation.

### Phase 2: Process management portability

Replace `process_group(0)` and `libc::kill` with platform-abstracted equivalents.
`child.try_wait()` already works. Runtime directory selection via `#[cfg]`.

### Phase 3: cfg gate cleanup and CI

Remove `#[cfg(unix)]` from `mod lsp`, `run_lsp_command`, and `main.rs` dispatch.
Add Windows CI runner for build + unit tests. Integration tests may require
a Windows rust-analyzer installation.

## Blockers

- No Windows test environment available.
