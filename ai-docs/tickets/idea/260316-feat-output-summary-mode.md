---
title: "summary subcommand — module TOC with item counts"
status: idea
---

# Summary Subcommand

## Priority: P2

## Motivation

Large crates produce thousands of lines with no progressive disclosure:
- axum: 2,220 lines (default), 438 lines (compact)
- tokio: 1,123 lines
- http: 2,324 lines

An LLM exploring an unfamiliar crate needs a 20-30 line overview first,
then targeted drill-down. Currently the only option is reading the full
output or using `search` (which requires knowing what to search for).

## Design (confirmed)

New subcommand: `cargo brief summary <target> [OPTIONS]`

### Output format

```
// crate tokio v1.38.0
mod io;                 // 4 traits, 15 structs, 8 fns
mod io::util;           // 12 structs, 6 fns
mod sync;               // 2 traits, 8 structs, 3 fns
mod sync::mpsc;         // 4 structs
mod task;               // 2 structs, 3 fns
mod time;               // 3 structs, 3 fns
// root: 5 macros, 2 fns
```

- Counts only, no item names — keeps output predictable and compact
- Zero-count kinds omitted
- All visible submodules listed flat (not tree)
- Existing visibility system applied (reachable set, is_visible_from)
- `pub use hidden::*` items counted at the re-exporting module level

### Target resolution

Shares the same target resolution as `api`/`search`:
- `cargo brief summary bevy::ecs` → crate bevy, module ecs
- `cargo brief summary --crates tokio` → remote crate
- `cargo brief summary self` → current package

### Shared options

`--crates`, `--features`, `--toolchain`, `-v`, `--manifest-path` — same as
other subcommands.

## Prerequisites

- ~~`--crates` positional arg `crate::module` parsing~~ (fixed: 39ad132)

## Complexity

Medium. New `summary.rs` module + `SummaryArgs` in cli.rs + pipeline
in lib.rs. No changes to existing api/search/examples pipelines.
