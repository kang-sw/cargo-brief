---
title: "Cross-crate search returns 0 results for tokio::spawn"
status: todo
---

## Problem

`cargo brief -C search tokio@1 spawn` returns 0 results.

```
$ cargo brief -C search tokio@1 spawn
// crate tokio — search: "spawn" (0 results)
```

`tokio::spawn` is one of the most prominent public APIs in the crate.
It's re-exported from `tokio::task::spawn` and should be discoverable.

## Likely Cause

Cross-crate search may not follow re-exports from internal sub-crates
when building the search index. The `CrossCrateIndex` might only index
types/traits but not free functions, or the search walker doesn't reach
items re-exported through `pub use` chains.

## Found By

Usability test (Q03) — quality evaluation of remote crate search.

## Acceptance Criteria

- `cargo brief -C search tokio@1 spawn` finds `tokio::spawn` and
  `tokio::task::spawn_blocking`
- Cross-crate search works for re-exported free functions, not just types
- No regression in existing `cargo test`
