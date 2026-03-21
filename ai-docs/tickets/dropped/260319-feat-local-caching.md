---
title: "Local pipeline caching — rustdoc JSON skip + bincode cache"
status: dropped
---

## Summary

Apply the remote pipeline's caching strategy to local crates: skip `cargo rustdoc`
when JSON already exists, cache parsed JSON as bincode for faster reloads.

## Motivation

Local crate queries feel sluggish. The dominant cost is `cargo +nightly rustdoc`
which runs on every invocation even when source hasn't changed. The remote pipeline
already has `generate_rustdoc_json_cached` + `parse_rustdoc_json_cached` that skip
regeneration and use bincode — local doesn't.

## Open Questions (need investigation)

- **Invalidation**: Remote crates are immutable (pinned version), so "file exists = valid"
  works. Local crates change constantly. What invalidation signal to use?
  Options: mtime of `Cargo.lock` + source files, content hash, cargo fingerprints.
- **Correctness risk**: Stale cache could show outdated API. Must be easy to bust
  (e.g., `--no-cache` flag, or automatic invalidation).
- **Bottleneck profile**: Is `cargo rustdoc` actually the bottleneck, or is JSON
  parsing / model building significant too? Need `--verbose` timing or `cargo flamegraph`.
- **Workspace interaction**: In a workspace, changing crate A may affect crate B's
  rustdoc output (cross-crate re-exports). Cache scope needs to account for this.

## Possible Approach

1. Profile with `--verbose` to measure time per pipeline stage
2. If rustdoc dominates: hash `Cargo.lock` + `src/**/*.rs` mtimes → skip if unchanged
3. If parsing dominates: bincode cache keyed on JSON file mtime
4. Both could be combined (remote pipeline already does #3)

## Risks

- False cache hits → stale output, user confusion
- Cache invalidation complexity may not justify the gain for small crates
- `cargo rustdoc` itself does incremental compilation — it may already be fast
  on unchanged code (need to measure cold vs warm)

## Partial Resolution (2026-03-21)

Cross-crate glob expansion now uses `use_cache: true` for non-workspace-member
crates. `PipelineContext.workspace_members` (from `cargo metadata`) identifies
mutable crates; anything outside that set is treated as immutable (locked via
Cargo.lock). This eliminates redundant `cargo rustdoc` calls for external dep
sub-crates during local facade traversal.

**Update (46dc0f3):** Local context builders now set `use_cache` based on
workspace membership. Non-member primary targets (e.g. `cargo brief search bevy`
from a game project) also skip regeneration when JSON exists.

Remaining: caching for the **target workspace member itself** and for
**workspace-member cross-deps**. Both require source-change detection
(mtime or hash-based invalidation).
