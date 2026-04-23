---
title: "summary: surface crate-root re-exports as a distinct section"
---

# summary: surface crate-root re-exports as a distinct section

## Background

The `summary` subcommand produces a module-level table of contents with item counts per
module. Crates that re-export items at the crate root (e.g. `pub use inner::Type` or
`pub use inner::*`) today contribute their counts to the `// root:` line but provide no
indication that the root-level items originate from re-exports rather than being defined
there.

For facade crates (bevy, axum, serde) this matters: the crate root IS the primary API
surface and consists entirely of re-exports from sub-crates. A summary that shows only
`// root: 1 trait, 5 structs` tells the user nothing about where those items came from.

The proposed enhancement: distinguish re-exported items from defined items at root level,
and optionally list the re-export origins.

## Constraints

- **Implementation difficulty.** This feature has stalled in prior exploration. The
  challenge is that glob re-exports from the root can expand to hundreds of items, and the
  summary format is designed to be compact. Showing per-item origins would make the output
  as long as the `api` output for large crates.
- **Overlap with cross-crate facade expansion.** The `api` subcommand already follows
  re-exports and assigns accessible paths. Reusing that machinery for summary-level
  aggregation is the likely implementation path, but it adds significant complexity to a
  subcommand that was intentionally lightweight.
- **Scope creep risk.** A "re-export origin" line per re-exported source module could
  easily become a mini-api output. The design must enforce a hard summary/detail boundary.

## Decisions

No design decisions made. Deferred to when the idea is promoted to `todo/`.

At promotion time, the key scope question: show re-export *counts by source module* (still
summary-level) vs show re-export *item names* (api-level detail that doesn't belong here).
