---
title: "Tree-sitter structural query subcommand"
status: wip
started: 2026-03-22
---

# Tree-sitter structural query subcommand (`ts`)

## Summary

Add `cargo brief ts <target> '<query>'` — runs a tree-sitter S-expression
query against crate source files and outputs matching nodes. Addresses the
only remaining gap in cargo-brief's coverage: structural code search with
multiline/AST-level precision that regex grep cannot provide.

## Motivation

- AI agents need structural code queries (e.g., "find all impl blocks for
  trait X", "extract struct literals with specific fields") that regex grep
  cannot reliably handle, especially with multiline patterns.
- Rust's formatting conventions split items across lines frequently — single-line
  regex misses many patterns.
- Current `examples` grep is regex-only; `Grep` tool with `multiline: true`
  exists but is fragile for structural patterns.
- Primary consumer is the project author (dogfooding with Claude Opus/Sonnet).

## Design

### CLI

```
cargo brief ts <target> '<query>' [OPTIONS]
```

Separate subcommand (not under `examples`). Inherits shared options
(`--crates`, `--features`, `--toolchain`, etc.).

### Scope

Default: `src/` + `examples/` + `tests/` + `benches/` of the target crate.
Scope-limiting flags:

| Flag | Scope |
|------|-------|
| (default) | All source files |
| `--src-only` | Only `src/` |
| `--examples-only` | Only examples/tests/benches |

### Feature gate

- `tree-sitter` feature: **default** (primary user is the author, minimal friction).
- Dependencies: `tree-sitter`, `tree-sitter-rust`.
- If compile size becomes an issue, can be moved to non-default later.

### Output modes

All modes prefix matches with `file:line` annotations.

| Flag | Mode | Description |
|------|------|-------------|
| (default) | verbatim | Matched node source text, one per match |
| `--context N` | context | Matched node with N lines of surrounding context |
| `--captures` | structured | Capture name + source text pairs (for multi-capture queries) |

### Help documentation

`--help` must include comprehensive, AI-friendly documentation:
- Tree-sitter query syntax primer with Rust-specific examples.
- Common query patterns (find function defs, struct fields, impl blocks,
  trait impls, call expressions, match arms, etc.).
- Capture naming conventions and how they map to `--captures` output.
- Example invocations for typical AI agent workflows.

This is critical — the primary users (LLM agents) will read `--help` to
learn query syntax before writing queries.

### Performance

- tree-sitter is designed for editor-speed parsing: ~ms per file.
- Even large crates (~200 files) should be <1s sequentially.
- Initial implementation: single-threaded, file-by-file.
- Rayon parallelism as follow-up if needed (unlikely to be necessary).

### Dependencies

- `tree-sitter` core: ~300KB C, needs `cc` crate for compilation.
- `tree-sitter-rust`: ~200KB grammar.
- Grammar version may lag behind bleeding-edge Rust syntax; acceptable.

## Resolved Questions

- **Scope**: `src/` included — separate subcommand, not an `examples` extension.
- **Feature gate**: default-on (dogfooding use case, minimal build friction).
- **Output ordering**: by file then match position (natural reading order).

## Open Questions

- Exact subcommand name: `ts` vs `tree-sitter` vs `query`?
- Should `--crates` mode download source from crates.io registry, or only
  work with local/cached workspaces?
- Limit on number of matches (pagination like `search --limit`)?

## Implementation Phases

### Phase 1: Core subcommand (plan mode)

- Add `tree-sitter` and `tree-sitter-rust` dependencies (default feature)
- `TsArgs` struct in `cli.rs`
- `src/ts.rs` module: parse files, run query, format output
- `run_ts_pipeline()` in `lib.rs`
- Basic tests with test_fixture

### Phase 2: Polish

- `--help` with comprehensive query examples
- `--captures` output mode
- Scope-limiting flags
- Remote crate support (`-C`)
