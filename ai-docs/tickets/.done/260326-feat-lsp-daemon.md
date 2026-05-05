---
title: "LSP daemon integration via rust-analyzer"
status: done
started: 2026-03-26
completed: 2026-03-27
related:
  260326-feat-lsp-daemon-bootstrap: "sub-ticket: daemon bootstrap (done)"
  260326-feat-lsp-file-watcher: "sub-ticket: file watcher (done)"
  260326-feat-lsp-query-commands: "sub-ticket: query commands (done)"
  260327-refactor-lsp-fifo-ipc: "IPC refactor: UDS → FIFO (done)"
---

# LSP daemon integration via rust-analyzer

## Goal

Add `cargo brief lsp <command>` subcommand family that provides ground-truth
semantic code analysis (edit impact, references, call hierarchy) by managing a
persistent rust-analyzer process as a timeout-based background daemon.

## Architecture

```
cargo brief lsp <command> [args]
    │
    ├─ resolve workspace root (cargo metadata — existing logic)
    ├─ daemon alive? (PID file + UDS health ping)
    │   ├─ No → spawn daemon (implicit touch)
    │   │       ├─ start ra subprocess (LSP over stdio)
    │   │       ├─ LSP initialize (workspace root)
    │   │       ├─ start file watcher (notify: *.rs, Cargo.toml, Cargo.lock)
    │   │       ├─ listen on UDS
    │   │       └─ signal ready → client proceeds
    │   └─ Yes → connect via UDS
    │
    ├─ send query → daemon translates → LSP request → ra
    ├─ ra responds → daemon formats → pseudo-Rust output
    └─ done (daemon stays alive, idle timer reset)

    [idle timeout] → LSP shutdown → ra exit → daemon exit → cleanup PID/sock
```

### Key decisions

- **One daemon per Cargo workspace root.** ra indexes an entire workspace;
  this matches cargo-brief's existing `cargo metadata` resolution.
- **Socket location:** `$XDG_RUNTIME_DIR/cargo-brief/{hash}/lsp.sock`
  (or `$TMPDIR` fallback). No workspace pollution.
- **All `lsp` commands implicitly ensure the daemon is running** (auto-touch).
  Explicit `lsp touch` is for pre-warming only.
- **File watcher in daemon** (via `notify` crate). Translates FS events to
  `workspace/didChangeWatchedFiles` LSP notifications. Eliminates need for
  manual refresh. Watch targets: `*.rs`, `Cargo.toml`, `Cargo.lock`.
  Excludes: `target/`, hidden dirs.
- **Idle timeout** (default ~10min). Configurable via env or flag.
- **ra binary discovery:** `rust-analyzer` on PATH, or `rustup which rust-analyzer`.
  Graceful error if not installed.
- **Opt-in:** `lsp` subcommand adds no dependency cost when unused.
  Feature-gate `notify` and LSP client code behind `lsp` cargo feature if
  binary size becomes a concern.

### CLI surface

```
cargo brief lsp touch                       # pre-warm daemon
cargo brief lsp stop                        # graceful shutdown
cargo brief lsp status                      # daemon alive? ra state?
cargo brief lsp references <symbol>         # find all references
cargo brief lsp blast-radius <symbol>       # edit impact analysis
cargo brief lsp call-hierarchy <symbol>     # incoming/outgoing calls
```

### Workspace isolation

- Daemon keyed to workspace root (hash of canonical path).
- Multiple workspaces = multiple independent daemons.
- Remote (`-C`) workspaces: use cached workspace path as root. Low priority.

### What multi-root workspace means (and why we skip it)

LSP multi-root workspace is a protocol feature where one server indexes
multiple unrelated project folders (e.g., two repos in one VS Code window).
Not relevant here — cargo-brief always operates on one Cargo workspace at
a time. No edge cases lost.

## Sub-tickets

1. `260326-feat-lsp-daemon-bootstrap` — Daemon process, UDS, PID, ra spawn,
   LSP initialize/shutdown, touch/stop/status commands
2. `260326-feat-lsp-file-watcher` — notify integration, didChangeWatchedFiles
3. `260326-feat-lsp-query-commands` — references, blast-radius, call-hierarchy
   + pseudo-Rust output formatting

## Estimated scope

~2000-2500 lines total across sub-tickets. Roughly 30% of current codebase.

## Out of scope (for now)

- Remote crate (`-C`) LSP analysis
- In-process ra embedding (library mode)
- Auto-refactoring / code actions
- Diagnostics forwarding

### Result (d620626) - 26-03-27

All sub-tickets complete. Full `cargo brief lsp` command family operational:
touch, stop, status, references, blast-radius, call-hierarchy.

Architecture note: the UDS-based IPC described in this ticket's Architecture
section was replaced by FIFO pair + flock serialization in
`260327-refactor-lsp-fifo-ipc` for macOS sandbox compatibility. The current
implementation uses `lsp.req`/`lsp.resp` named pipes instead of `lsp.sock`.
See `ai-docs/mental-model/lsp-daemon.md` for accurate current architecture.
