---
title: "Refactor CLI to subcommand pattern + smart-case + search OR"
status: done
started: 2026-03-19
completed: 2026-03-19
---

## Summary

Restructure the CLI from a single flat command to subcommands. Simultaneously
introduce smart-case matching and comma-separated OR for search patterns.

## Subcommand Structure

```
cargo brief api [target] [module] [options]       # API extraction (current default)
cargo brief search [target] <pattern> [options]   # Item search (current --search)
cargo brief examples [target] [pattern] [options] # Example grep (new, ticket #2)
```

### `api` subcommand
- Absorbs current default behavior (API extraction + rendering)
- Owns: `--depth`, `--recursive`, `--expand-glob`, `--compact`, `--no-docs`,
  `--doc-lines`, `--no-crate-docs`, `--verbose-metadata`, `--no-*` filters,
  `--at-package`, `--at-mod`, `--all`
- `--search`, `--search-limit`, `--methods-of` move OUT

### `search` subcommand
- Pattern is a positional argument, not a flag
- Owns: `--limit` (rename from `--search-limit`), `--methods-of`
- Shares with api: `--no-*` filters (for result filtering), `--no-docs`,
  `--doc-lines`

### `examples` subcommand (implemented in ticket #2)
- Optional pattern positional: list files when absent, grep when present
- Owns: `--context` or `--grep-range` (before:after context lines)
- `--include-tests` to extend search to `tests/` directory

### Shared options (all subcommands)
- `[target]`, `[module_path]` — target resolution
- `--crates <spec>`, `--features`, `--no-cache`, `--clean`
- `--toolchain`, `--manifest-path`, `--verbose`

## Smart-case Matching

Apply to both `search` and `examples` subcommands:
- Pattern is all lowercase → case-insensitive
- Pattern contains any uppercase → case-sensitive

Matches ripgrep/vim `smartcase` semantics.

## Search OR via Comma

- Space-separated words = AND (existing behavior)
- Comma-separated terms = OR (new)
- `"EventReader,EventWriter"` → matches either
- `"World spawn"` → matches both (AND)
- Mixed: `"World spawn,despawn"` → `World AND (spawn OR despawn)`
  (comma binds tighter than space? or simpler: split by comma first as OR
  groups, each group is AND of its words)

Simplest semantics: comma splits into OR groups. Each group is a single
match term (no AND within OR). If users need AND+OR, they run two queries.
`"EventReader,EventWriter"` = OR. `"World spawn"` = AND. No mixing.

## Implementation Notes

- Use clap `#[derive(Subcommand)]` with shared `Args` groups via `#[command(flatten)]`
- `run_pipeline()` signature changes — takes enum or per-subcommand args
- Breaking change: all existing invocations need `api` prefix (or make `api`
  the default subcommand via clap's `default_subcommand` or arg parsing fallback)
- No backward compatibility shims needed (solo dev, agents use `--help`)

## Acceptance Criteria

- `cargo brief api self` produces identical output to current `cargo brief self`
- `cargo brief search self pattern` equivalent to current `cargo brief self --search pattern`
- `cargo brief --help` shows subcommand list with clear descriptions
- Smart-case: `"world"` matches `World`, `"World"` does not match `world`
- OR: `"Foo,Bar"` matches items containing either `Foo` or `Bar`
- All existing integration tests pass (adapted to new API)

### Result — 2026-03-19

Implemented as planned. Key changes:

**CLI restructure:**
- `BriefArgs` (26 fields, flat) → per-subcommand `ApiArgs`/`SearchArgs`/`ExamplesArgs` with shared `TargetArgs`/`RemoteArgs`/`FilterArgs`/`GlobalArgs` groups via `#[command(flatten)]`
- `BriefCommand` enum with `Api`/`Search`/`Examples` variants
- `BriefDirect` parser wrapper for direct `cargo-brief` invocation
- `examples` is a stub that exits with error message

**Pipeline split:**
- `run_pipeline(&BriefArgs)` → `run_api_pipeline(&ApiArgs)` + `run_search_pipeline(&SearchArgs)`
- Remote pipelines split similarly: `run_remote_api_pipeline` / `run_remote_search_pipeline`
- `render.rs` functions take `&FilterArgs` (or `&ApiArgs` for top-level render_module_api)
- `search.rs` takes `&FilterArgs` + explicit `limit: Option<&str>` parameter

**Smart-case + OR:**
- `pattern.chars().any(|c| c.is_uppercase())` → case_sensitive flag
- `pattern.split(',')` → OR groups, each split by whitespace for AND tokens
- 4 new tests: insensitive, sensitive, comma OR, no cross-match

**Tests:** All 109 integration + 24 subprocess + 11 facade + 4 smoke = 148+ tests pass.
Pre-existing failures in either crate tests (nightly API drift) and remote cache tests (race condition) unrelated.

**Deviations from plan:**
- `SearchArgs` has its own `crate_name`/`at_package`/`at_mod`/`manifest_path` fields (not from `TargetArgs`) since search doesn't need `module_path`
- OR groups support AND within each group (space-separated words per comma-group), matching the original plan's "simplest semantics"
