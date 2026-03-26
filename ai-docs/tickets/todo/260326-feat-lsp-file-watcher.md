---
title: "LSP daemon: file watcher integration"
---

# LSP daemon: file watcher integration

**Parent:** `260326-feat-lsp-daemon`

## Goal

Add filesystem watching to the LSP daemon so that rust-analyzer's VFS stays
current without manual refresh. File changes made by the AI agent (or user)
between queries are automatically reflected in subsequent analysis results.

## Dependencies

- `260326-feat-lsp-daemon-bootstrap` (daemon must exist)

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
