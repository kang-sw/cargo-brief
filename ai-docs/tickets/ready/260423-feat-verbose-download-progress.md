---
title: "verbose mode: progress messages for crates.io HTTP fetch phase"
spec:
  - 260423-verbose-remote-progress
---

# verbose mode: progress messages for crates.io HTTP fetch phase

## Background

With `--verbose`, cargo-brief already reports batch rustdoc generation and cache hits (see
spec `260423-verbose-remote-progress`). What is currently silent: the HTTP calls made
during version resolution — the crates.io API fetch that happens before cargo is invoked.

For a user running `cargo brief -C -v api tokio@1` for the first time, there is a
multi-second pause with no output while the crates.io API is queried. This looks like a
hang. The fix is to emit a progress line to stderr before the fetch and a confirmation
after.

**Note on scope:** cargo-brief does not directly download `.crate` tarballs — that is
handled by cargo itself, whose stderr is already inherited and streamed in verbose mode.
This ticket covers only the cargo-brief-owned HTTP calls (version resolution, feature
graph fetch), not cargo's own download phase.

## Phases

### Phase 1: Emit verbose messages around crates.io API calls

**Goals:**
- Before the version-resolution HTTP call: emit to stderr
  `[cargo-brief] Resolving version for '{name}'…`
- After a successful fetch and write to version cache: emit
  `[cargo-brief] Resolved '{name}' → {version} (cached)`
- After a cache hit (version cache still valid): emit
  `[cargo-brief] Version cache hit for '{name}' ({version})`
- On API failure with stale cache fallback: the existing warning covers this; no change.
- All messages gated on `--verbose` / `-v`; silent otherwise.

**Constraints:**
- Messages go to stderr only, never stdout.
- Format must match the existing `[cargo-brief]` prefix convention used by batch-doc
  messages.
- Applies to both version resolution and feature graph fetch (both are crates.io API calls).

**Success criteria:**
- `cargo brief -C -v api serde` emits at least one `[cargo-brief] Resolv...` line before
  any cargo output appears.
- Without `-v`, no new lines appear.
- Existing verbose messages are unaffected.
