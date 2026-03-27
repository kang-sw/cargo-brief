---
title: "LSP daemon: track rust-analyzer indexing status via $/progress"
related:
  - 260326-feat-lsp-windows-support  # same subsystem
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
