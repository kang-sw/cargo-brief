---
title: "Multi-pattern positional args for search/examples"
status: wip
started: 2026-03-21
---

# Multi-pattern positional args for search/examples

## Goal

Allow multiple positional arguments as pattern input instead of requiring
a single quoted string. Purely ergonomic improvement.

```sh
# Before (requires quotes for multi-word AND pattern)
cargo brief search bevy "ShaderRef Material"

# After (bare args joined with space → AND semantics preserved)
cargo brief search bevy ShaderRef Material
```

## Scope

- `SearchArgs.pattern`: single `String` → `Vec<String>`, joined with space
- `ExamplesArgs.pattern`: single `Option<String>` → `Vec<String>`, joined with space
  (empty vec = list mode)
- OR semantics (comma) unchanged — still within a single arg token
- No behavioral change for existing single-arg usage

## Design

clap: first positional = TARGET (single String), remaining positionals = patterns
(`Vec<String>`). Internally join with `" "` before passing to matching logic.

For `SearchArgs`, the current `default_value = ""` on pattern becomes unnecessary —
empty Vec means no pattern (used with `--methods-of` alone).

## Complexity

Low. CLI arg type change + join at call site. No pipeline changes.
