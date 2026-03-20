# Overview

## Entry Points
- `src/lib.rs` — `run_api_pipeline(&ApiArgs)`, `run_search_pipeline(&SearchArgs)`, and `run_examples_pipeline(&ExamplesArgs)` are the three pipeline entry points; start here.
- `src/main.rs` — CLI dispatch only: parses `BriefCommand` enum and dispatches to the three pipeline functions (dual invocation: `cargo brief <sub>` vs `cargo-brief <sub>`).

## Module Contracts
- `lib.rs` guarantees: three public pipeline functions. Both `run_api_pipeline` and `run_search_pipeline` follow the same two-phase structure: (1) build a `PipelineContext` via `build_local_context_*` or `build_remote_context_*`, then (2) call the shared `run_shared_api_pipeline` / `run_shared_search_pipeline`. Both local and remote paths get cross-crate discovery through the shared pipeline. `run_examples_pipeline` is disk-only — no rustdoc JSON, no model building; it reads `.rs` files directly from `examples/`, `tests/`, and `benches/` directories. No stage within a path may be reordered.
- `resolve`, `rustdoc_json`, `remote`, and `cross_crate` are pure utilities with zero internal dependencies on each other. They can be tested in isolation.
- `model` depends only on `rustdoc_types` (external). `render` depends on `model` + `cli`.
- `examples` depends only on `cli` (for `ExamplesArgs`). It has no dependency on `model`, `render`, `rustdoc_json`, or `resolve`.
- `lib.rs` is the sole orchestrator — all cross-module data flow passes through it.
- `cross_crate` depends on `rustdoc_json` (for `generate_rustdoc_json(..., true)` / `parse_rustdoc_json_cached`) and `model`. It never calls `remote` or `resolve`.

## Coupling
- `render` → `lib.rs`: Glob re-export output format must match exactly. `render_module_api()` emits `pub use {source}::*;\n` for top-level globs (no indent); `apply_glob_expansions()` searches for this exact string without indentation. Indented globs (from deeper modules) would not match — this coupling is fragile. Change either side without the other → globs silently remain unexpanded.
- `cli` → all test files: Test helpers construct `ApiArgs` by building all four flattened structs (`TargetArgs`, `RemoteArgs`, `FilterArgs`, `GlobalArgs`) plus the per-subcommand fields. Adding a field to any of these structs causes compile errors across all helpers — intentional, not silent.
- `lib.rs` → `resolve` + `rustdoc_json`: `manifest_path` is threaded through without validation. If it points to the wrong Cargo.toml, failure surfaces at JSON generation time, not at metadata loading.
- `cross_crate` → `rustdoc_json` caching: `cross_crate` calls `generate_rustdoc_json(..., use_cache=true)` and `parse_rustdoc_json_cached`. `expand_glob_reexports` threads `use_cache` from `PipelineContext`, so local pipelines pass `false` (always regenerate) and remote pipelines pass `true` (skip if JSON exists). All callers share the same `target/doc/` directory — `.bin` cache files accumulate there and are never cleaned by `--clean`.

## Extension Points & Change Recipes
- **Add a new `--no-*` filter flag**: Touch `cli.rs` (`FilterArgs` struct), `render.rs` (`should_render_item`), all test helpers (`default_filter()` in each file). Compile errors guide you.
- **Add a new item type**: Touch `render.rs` (add renderer + visibility check), `test_fixture/src/lib.rs` (add example), `tests/integration.rs` (add assertion). Missing the visibility check → private items leak silently.

## Common Mistakes
- Calling `render_item()` for a new item type without a preceding `is_visible_from()` check → private items appear in output.

## Technical Debt
- String-based glob detection/replacement in `apply_glob_expansions` — fragile, first-occurrence-only semantics. See `glob-expansion.md`.
- `--verbose` / `-v` prints progress to stderr; messages are in `lib.rs` only (not in utility modules).
- Cross-crate hop limit is hardcoded at 5 in `cross_crate.rs`. Deep facade chains (>5 hops) silently fall back to "module not found".
