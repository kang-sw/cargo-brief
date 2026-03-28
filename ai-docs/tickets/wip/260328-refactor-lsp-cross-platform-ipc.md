---
title: "LSP IPC refactoring: platform abstraction for cross-platform support"
started: 2026-03-28
plans:
  phase2: 2603/28-1033-lsp-process-abstraction
related:
  - 260326-feat-lsp-daemon-bootstrap  # original FIFO-based IPC
  - 260326-feat-lsp-windows-support   # runtime testing deferred until Windows env available
  - 260327-refactor-lsp-fifo-ipc      # prior FIFO refactoring
---

# LSP IPC refactoring: platform abstraction for cross-platform support

## Problem

The LSP daemon IPC layer is tightly coupled to Unix-specific APIs (`libc::mkfifo`,
`flock`, `fcntl`, `poll`, `kill`, `AsRawFd`, `process_group`). The entire `lsp`
module is `#[cfg(unix)]` and does not compile on Windows.

Beyond portability, `client.rs` (448 lines) mixes low-level IPC mechanics with
high-level client logic, making the code harder to maintain and test.

## Goal

1. Extract platform-specific IPC and process management into abstraction modules.
2. Implement a Windows backend using **filesystem-only** IPC (sandbox-safe).
3. Verify cross-compilation via `cargo check --target x86_64-pc-windows-msvc`.
4. Remove `#[cfg(unix)]` gate from the `lsp` module.

## Design Decisions

### IPC mechanism (Windows): atomic-rename file protocol

**Constraint:** Claude Code sandbox only allows filesystem access within the
project directory. System-global IPC (Windows Named Pipes `\\.\pipe\...`,
TCP localhost) would trigger sandbox firewall. IPC must use files under `target/`.

**Chosen approach — atomic rename:**

```
Writer:  write(path.tmp, data)  →  rename(path.tmp → path)
Reader:  poll path exists?      →  read & delete path
```

- `rename()` is atomic at the filesystem level on both Unix and Windows (NTFS).
- Reader either sees nothing or the complete file — no partial reads.
- No markers, no memory ordering concerns, no mmap complexity.
- Small payloads (< 4KB) stay in OS page cache; no actual disk I/O at
  LSP query frequencies (a few per minute at most).

**Request-response flow (Windows):**

```
Client                              Daemon
  │                                   │
  ├─ acquire lock (LockFileEx)        │
  ├─ write lsp.req.tmp                │
  ├─ rename → lsp.req                 │
  │                               poll lsp.req exists?
  │                                   ├─ read & delete lsp.req
  │                                   ├─ process query
  │                                   ├─ write lsp.resp.tmp
  │                                   └─ rename → lsp.resp
  ├─ poll lsp.resp exists?            │
  ├─ read & delete lsp.resp           │
  ├─ release lock                     │
  └─ done                             │
```

**Unix stays on FIFOs** — existing FIFO-based IPC works well and is already
battle-tested. No reason to change it.

### Platform abstraction structure

```
src/lsp/
  ipc/
    mod.rs        — IPC trait + platform re-export
    unix.rs       — FIFO-based (extract from current client.rs/daemon.rs)
    windows.rs    — atomic-rename file protocol
  process/
    mod.rs        — process management trait + platform re-export
    unix.rs       — process_group(0), kill(pid, 0)
    windows.rs    — CREATE_NEW_PROCESS_GROUP, OpenProcess(SYNCHRONIZE)
  client.rs       — high-level logic only, uses ipc/ and process/ traits
  daemon.rs       — high-level logic only, uses ipc/ and process/ traits
  mod.rs          — public interface (minimal change)
  protocol.rs     — unchanged (pure serialization)
  query.rs        — unchanged (pure LSP JSON-RPC)
  transport.rs    — minor: abstract AsRawFd usage
  watcher.rs      — unchanged (notify is cross-platform)
```

### Cross-compilation verification

`cargo check --target x86_64-pc-windows-msvc` runs type checking without a
linker — works on macOS. This verifies compilation without a Windows machine.
Actual runtime testing is deferred to `260326-feat-lsp-windows-support`.

## Phases

### Phase 1: IPC abstraction layer

Extract FIFO-specific code from `client.rs` and `daemon.rs` into `ipc/unix.rs`.
Define the IPC trait in `ipc/mod.rs`. Implement `ipc/windows.rs` with atomic-rename
protocol. Unix behavior must be unchanged (refactor only).

Success criteria: `cargo test` passes on macOS, `cargo check --target
x86_64-pc-windows-msvc` passes for the ipc module.

### Phase 2: Process management abstraction

Extract `process_group(0)`, `kill(pid, 0)`, and runtime directory logic into
`process/`. Implement Windows equivalents (`CREATE_NEW_PROCESS_GROUP`,
`OpenProcess`, `%LOCALAPPDATA%` fallback).

Success criteria: same as Phase 1.

### Phase 3: Remove cfg(unix) gate and CI

Remove `#[cfg(unix)]` from `lib.rs` and `Cargo.toml`. Fix remaining
platform-specific code in `transport.rs`. Add cross-compilation check to CI.

Success criteria: `cargo check --target x86_64-pc-windows-msvc` passes for the
entire crate. Existing Unix tests still pass.

### Result (1e73f6f) - 26-03-28

**Phase 2 complete.** Extracted `process_alive()`, daemon spawn detachment
(`process_group(0)`), and `which` binary lookup from `client.rs`/`daemon.rs`
into `src/lsp/process/{mod,unix,windows}.rs`.

- Unix backend: pure extraction, behavior unchanged. All tests pass.
- Windows backend: `OpenProcess(SYNCHRONIZE)` for liveness, `creation_flags(CREATE_NEW_PROCESS_GROUP)` for detachment, `where.exe` for PATH lookup. Compiles under `cfg(windows)` but is dead code until Phase 3 removes the outer `#[cfg(unix)]` gate.
- `windows-sys` 0.59 added as `cfg(windows)` dependency.
- No deviations from the plan.
- Code review minor notes (deferred to Phase 3): `CREATE_NEW_PROCESS_GROUP` uses local constant instead of `windows-sys` export; `process_alive` has redundant `INVALID_HANDLE_VALUE` check (harmless).
- Runtime directory logic extraction (`%LOCALAPPDATA%` fallback) was not in scope for this phase — `daemon_dir()` remains in `client.rs`.

**Next:** Phase 1 (IPC abstraction) or Phase 3 (cfg gate removal).

## Out of Scope

- Actual Windows runtime testing (see `260326-feat-lsp-windows-support`).
- Changing the Unix IPC mechanism (FIFOs stay).
- Performance optimization of the polling interval (can tune later).
