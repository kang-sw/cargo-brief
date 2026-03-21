---
title: "Tree-sitter query subcommand (example-ts)"
status: idea
---

# Tree-sitter query subcommand (`example-ts`)

## Summary

Add `cargo brief example-ts <target> '<query>'` — runs a tree-sitter S-expression
query against example/test/bench source files and outputs all matching nodes.
Feature-gated behind `--features tree-sitter` to keep the default binary lean.

## Motivation

AI agents need structural code queries (e.g., "find all function calls to X",
"extract all struct literals") that regex grep cannot reliably provide.
Tree-sitter queries give AST-level precision without full compilation.

## Design

### CLI

```
cargo brief example-ts <target> '<query>' [OPTIONS]
```

Inherits shared options (`--crates`, `--features`, `--toolchain`, etc.) and
example-scoping options (`--tests [DEPTH]`, `--benches [DEPTH]`).

### Feature gate

- Cargo feature: `tree-sitter` (not default).
- Dependencies: `tree-sitter`, `tree-sitter-rust` (behind the feature).
- Subcommand hidden/unavailable when compiled without the feature.

### Output modes

Selectable via flags. All modes prefix matches with `file:line` annotations.

| Flag | Mode | Description |
|------|------|-------------|
| (default) | verbatim | Matched node source text, one per match |
| `--context N` | context | Matched node with N lines of surrounding context |
| `--captures` | structured | Capture name + source text pairs (for multi-capture queries) |

### Help documentation

`--help` must include comprehensive, AI-friendly documentation:
- Tree-sitter query syntax primer with Rust-specific examples.
- Common query patterns (find function defs, struct fields, impl blocks, etc.).
- Capture naming conventions and how they map to output in `--captures` mode.
- Example invocations for typical AI agent workflows.

### Dependencies

- `tree-sitter` core: ~300KB C, needs `cc` crate for compilation.
- `tree-sitter-rust`: ~200KB grammar.
- Grammar version may lag behind bleeding-edge Rust syntax; acceptable for
  reading example/test code.

## Open questions

- Should the query also run against the crate's own `src/` files, or strictly
  example/test/bench? Restricting to examples matches the `examples` subcommand
  scope, but `src/` querying could be useful.
- Output ordering: by file then match position, or grouped by capture name?
