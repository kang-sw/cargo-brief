---
title: "Tree-sitter structural query subcommand"
status: done
started: 2026-03-22
completed: 2026-03-22
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

### Phase 1.5: Usability fixes (from Haiku/Sonnet testing)

LLM usability tests revealed two issues:

1. **Verbatim mode shows first capture, not pattern root node.**
   Query `(impl_item trait: (type_identifier) @t (#eq? @t "MyTrait"))`
   outputs just `"MyTrait"` instead of the full impl block. Fix: auto-augment
   every top-level pattern with `@_match` (extending existing capture-less
   augmentation), then prefer `@_match` in verbatim mode.

2. **`--context` + `--captures` silently ignored.** Emit stderr warning.

Implementation: ~40 lines in `ts.rs` (S-expression pattern boundary detection
+ capture selection logic). No new files, no API changes.

### Phase 2: Polish

- Comprehensive `--help` examples (more query patterns, Rust-specific tips)
- Scope-limiting flags (`--src-only`, `--examples-only`)
- Remote crate support (`-C`)
- `--limit` pagination

### Result (c6715e8) - 26-03-22

**Phase 1 completed.** Implemented:
- `cargo brief ts <target> '<query>'` subcommand with verbatim, `--captures`, and `--context` output modes
- Always-on deps: `tree-sitter 0.25`, `tree-sitter-rust 0.23`, `streaming-iterator 0.1`
- `TsArgs` in `cli.rs`, `BriefCommand::Ts` variant, `run_ts_pipeline()` in `lib.rs`, dispatch in `main.rs`
- `src/ts.rs` module (~140 lines): `collect_source_files`, `run_query`, `render_with_context`
- Reuses `examples::collect_rs_files` (promoted to `pub(crate)`) and `examples::parse_context`
- Capture-less queries auto-augmented with `@_match` (caught in code review)
- Remote crate support returns clear error message
- 7 integration tests added (183 total)

**Deviations from plan:** `--captures` mode included in Phase 1 (simpler than expected). Moved from Phase 2.

**Key findings:**
- tree-sitter `QueryMatches`/`QueryCaptures` use `StreamingIterator` — explicit dep required
- Capture-less queries need auto-augmentation; S-expression queries without `@capture` produce empty captures array

### Result - 26-03-22

**Phase 2 completed.** Implemented:
- `--src-only`: restricts file scanning to `src/` only (skips examples/tests/benches)
- `--limit [OFFSET:]N`: pagination with early-exit via labeled `'files:` break. `parse_limit()` helper in `ts.rs`
- `--quiet`/`-q`: location-only output (`@file:line`), compatible with `--captures` and `--limit`
- Remote `-C` support: follows `run_examples_pipeline` pattern — `resolve_workspace` + `find_dep_source_root` + `run_query`. `WorkspaceDir` binding keeps workspace alive through query execution
- Comprehensive `--help`: node type reference, capture semantics, predicate reference, practical tips, playground link
- No-matches output enhanced with playground URL hint
- 7 new integration tests (191 total): src_only exclusion/inclusion, limit, limit+offset, quiet, quiet+captures, no-matches hint

**Deviations from plan:** `--examples-only` deferred as planned — `--src-only` covers 90% of use cases.

**Key findings:**
- Limit logic uses `match_count` (total seen) for offset skip and `emitted` (actually written) for limit check — clean separation
- Quiet mode in captures branch still emits `@file:line` but omits `@name: text` lines
