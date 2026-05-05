---
title: "LSP daemon: file watcher integration"
status: done
parent: 260326-feat-lsp-daemon
completed: 2026-03-26
plans:
  phase1: 2026-03/26-1830-lsp-file-watcher
related:
  260326-feat-lsp-daemon: parent
  260326-feat-lsp-daemon-bootstrap: prerequisite
---

# LSP daemon: file watcher integration

## Goal

Add filesystem watching to the LSP daemon so that rust-analyzer's VFS stays
current without manual refresh. File changes made by the AI agent (or user)
between queries are automatically reflected in subsequent analysis results.

## Design

### Watch targets

| Pattern | Reason |
|---|---|
| `**/*.rs` | Source changes |
| `**/Cargo.toml` | Dependency / feature changes |
| `**/Cargo.lock` | Resolved version changes |

### Exclusions

- `target/` — build artifacts, high churn, no analysis value
- `.*` hidden directories — `.git/`, etc.
- Non-workspace paths

### Implementation

- Use `notify` crate (v6+) with `RecommendedWatcher`
- Debounce: 200-500ms (batch rapid saves into one notification)
- Translate `notify::Event` → LSP `workspace/didChangeWatchedFiles`:
  - `Create` → `FileChangeType::Created (1)`
  - `Modify` → `FileChangeType::Changed (2)`
  - `Remove` → `FileChangeType::Deleted (3)`
- Send as LSP notification (no response expected)

### Integration with daemon event loop

```
daemon loop:
  select! {
    uds_event = uds_listener.accept() => { handle_client(uds_event) }
    fs_event = watcher_rx.recv() => { notify_ra(fs_event) }
    _ = idle_timer.tick() => { if no_recent_activity { shutdown() } }
  }
```

## Acceptance criteria

- Modify a `.rs` file → subsequent `lsp references` reflects the change
  without restart
- Add/remove a file → ra picks it up
- `Cargo.toml` change (add dep) → ra re-indexes
- `target/` writes do not trigger notifications
- Debounce: rapid edits produce ≤1 notification batch

## Estimated scope

~200-300 lines (watcher setup + event translation + daemon integration)

## Dependencies (crate)

- `notify` 6.x (feature-gated if needed)

### Result (cf278fa) - 26-03-26

Implemented filesystem watching for the LSP daemon. New `src/lsp/watcher.rs`
(~160 lines) with `notify 6.1.1` under `[target.'cfg(unix)'.dependencies]`.

**What was built:**
- `start_watcher()`: `RecommendedWatcher` with recursive mode, mpsc channel bridge
- Event filtering: accept `.rs`/`Cargo.toml`/`Cargo.lock`, reject `target/`/hidden dirs
- `DebounceBuffer`: 300ms batching with URI dedup (latest change_type wins)
- `build_did_change_notification()`: LSP notification params construction
- Daemon integration: watcher starts after LSP initialize, FS events drained via
  `try_recv()` on every main loop iteration, graceful degradation on watcher failure

**Deviations from ticket design:**
- Ticket showed `select!`-based loop; actual implementation uses synchronous polling
  with `try_recv()` (matching the existing daemon architecture established in bootstrap)
- Debounce window: 300ms (within the 200-500ms range specified)

**Tests:** 17 unit tests (8 DebounceBuffer TDD, 7 event filtering post-impl,
2 notification format). All pass. Daemon integration tested manually.

**Remaining:** End-to-end validation that ra processes the notifications will be
covered by `260326-feat-lsp-query-commands` (next sub-ticket).
