---
title: "LSP daemon: track rust-analyzer indexing status via $/progress"
status: done
started: 2026-03-27
completed: 2026-03-27
plans:
  phase1: 2026-03/27-2130-lsp-indexing-status
related:
  260326-feat-lsp-windows-support: same subsystem
---

## Problem

The daemon sets `RaStatus::Ready` immediately after the LSP `initialize`
handshake, but rust-analyzer continues indexing in the background (cargo
workspace loading, proc-macro expansion, symbol indexing). During this
window, `workspace/symbol` returns empty results, causing
"Symbol not found" errors that are indistinguishable from genuinely
missing symbols.

Observed on large workspaces (8-member workspace with bevy/tokio deps):
indexing takes 10+ seconds after `initialize` completes.

## Goal

Track ra's actual indexing state so the daemon can:
1. Report accurate status (`Indexing` vs `Ready`).
2. On query during indexing: wait for completion (with timeout) rather
   than returning a false "not found".

## Design Decisions

- **Poll ra stdout in the main loop.** The daemon's main loop currently
  only polls the FIFO fd. Add ra's stdout fd to the `poll()` call so
  `$/progress` notifications are drained continuously.
- **`$/progress` token tracking.** rust-analyzer sends `begin`/`end`
  progress notifications. Track active progress tokens; `Indexing` state
  = any "loading" or "indexing" token is active. `Ready` = no active
  tokens remain after at least one begin/end cycle.
- **`RaStatus::Indexing` variant.** Insert between `Initializing` and
  `Ready`. Transition: `Initializing` → `Indexing` (on first progress
  begin) → `Ready` (all progress tokens ended).
- **Query-time wait.** When a query arrives during `Indexing`, the daemon
  waits (polling ra stdout for progress) up to a configurable timeout
  before executing the query. If timeout expires, return an explicit
  "indexing in progress" error rather than false "not found".

### Phase 1: ra stdout polling + indexing state tracking

Add ra stdout fd to the main loop poll set. Parse `$/progress`
notifications to maintain a set of active progress tokens. Derive
`RaStatus` from this set. Update `lsp status` output to show the
new state.

### Phase 2: query-time wait-for-ready

When a query request arrives and `ra_status == Indexing`, enter a
sub-loop that drains ra stdout until indexing completes or timeout
(default 60s, env-configurable). Then execute the query normally.
Update client-side `send_command` timeout to accommodate the extra wait.

### Result (56b7f2b) - 26-03-27

Implemented both phases in a single pass.

**Phase 1 — ra stdout polling + indexing state tracking:**
- Added `RaStatus::Indexing` variant to protocol.rs
- Added `stdout_raw_fd()`, `has_buffered_data()`, `send_raw_response()` to `RaTransport`
- Modified `send_request_and_wait()` to reply to server-initiated requests (e.g. `window/workDoneProgress/create`)
- Added `process_ra_notification()` pure function (11 unit tests) tracking `$/progress` begin/end tokens via `HashSet<String>`
- Added `drain_ra_messages()` called each main loop iteration with poll-then-read pattern
- `NO_PROGRESS_FALLBACK_SECS` (10s): if no `$/progress` ever seen, assume Ready
- Declared `window.workDoneProgress: true` in initialize capabilities (code review finding)

**Phase 2 — query-time wait-for-ready:**
- Added `wait_for_ready()` blocking drain loop (500ms poll intervals, 60s default timeout via `CARGO_BRIEF_LSP_READY_TIMEOUT`)
- Query commands (References, BlastRadius, CallHierarchy) gate on `wait_for_ready()` before dispatch
- Client-side query timeout increased to 120s (`QUERY_TIMEOUT` constant in mod.rs)

**Deviations from ticket:** Used single-fd poll pattern (poll ra stdout separately in main loop) rather than adding ra stdout to the FIFO pollfd set — simpler integration with existing BufReader wrapping.
