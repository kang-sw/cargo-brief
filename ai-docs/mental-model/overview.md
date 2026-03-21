# Overview

## Entry Points
- `src/lib.rs` — `run_api_pipeline`, `run_search_pipeline`, `run_examples_pipeline`, and `run_summary_pipeline` are the four pipeline entry points; start here.
- `src/main.rs` — CLI dispatch only: parses `BriefCommand` enum and dispatches to the four pipeline functions (dual invocation: `cargo brief <sub>` vs `cargo-brief <sub>`).

## Module Contracts
- `lib.rs` guarantees: four public pipeline functions. Both `run_api_pipeline` and `run_search_pipeline` follow the same two-phase structure: (1) build a `PipelineContext` via `build_local_context_*` or `build_remote_context_*`, then (2) call the shared `run_shared_api_pipeline` / `run_shared_search_pipeline`. Both local and remote paths get cross-crate discovery through the shared pipeline. `run_examples_pipeline` is disk-only — no rustdoc JSON, no model building; it reads `.rs` files directly from `examples/`, `tests/`, and `benches/` directories. `run_summary_pipeline` uses the same `PipelineContext` two-phase structure as api/search. No stage within a path may be reordered.
- `resolve`, `rustdoc_json`, `remote`, and `cross_crate` are pure utilities with zero internal dependencies on each other. They can be tested in isolation.
- `model` depends only on `rustdoc_types` (external). `compute_reachable_set()` returns `ReachableInfo` (with `reachable`, `glob_private_modules`, `glob_inlined`). `render` depends on `model` + `cli` + `cross_crate`. `search` and `summary` also depend on `cross_crate` for their index-based functions.
- `examples` depends only on `cli` (for `ExamplesArgs`). It has no dependency on `model`, `render`, `rustdoc_json`, or `resolve`.
- `lib.rs` is the sole orchestrator — all cross-module data flow passes through it.
- `cross_crate` depends on `rustdoc_json` (for `generate_rustdoc_json(..., true)` / `parse_rustdoc_json_cached`) and `model`. It never calls `remote` or `resolve`.
- `lib.rs` uses `cross_crate::collect_external_crate_names()` and the `pub(crate)` helpers `extract_crate_name` / `is_intra_crate_source` for pre-warming. For the "render all" pipelines (api/search/summary), `lib.rs` calls `cross_crate::build_cross_crate_index()` then passes the result to `render::render_cross_crate_api()`, `search::search_cross_crate_index()`, or `summary::summarize_cross_crate_index()`. Targeted module resolution (e.g. `cargo brief api bevy ecs`) uses `resolve_cross_crate_module()` → `CrossCrateResolution`.

## Coupling
- `render` → `lib.rs`: Glob re-export output format must match after whitespace normalization. `render_module_api()` emits `pub use {source}::*;`; `replace_glob_lines()` normalizes each line by collapsing whitespace before comparing to the pattern `pub use {source}::*;`. Indentation is preserved and re-applied to replacement lines. Any structural change to the glob line (extra tokens, different keyword order) → globs silently remain unexpanded.
- `cli` → all test files: Test helpers construct `ApiArgs` by building all four flattened structs (`TargetArgs`, `RemoteArgs`, `FilterArgs`, `GlobalArgs`) plus the per-subcommand fields. Adding a field to any of these structs causes compile errors across all helpers — intentional, not silent.
- `lib.rs` → `resolve` + `rustdoc_json`: `manifest_path` is threaded through without validation. If it points to the wrong Cargo.toml, failure surfaces at JSON generation time, not at metadata loading.
- `cross_crate` → `rustdoc_json` caching: `cross_crate` calls `generate_rustdoc_json(..., use_cache=true)` and `parse_rustdoc_json_cached`. Both `expand_glob_reexports`/`collect_glob_items_recursive` (used for glob-expansion phase) and `walk_accessible` (used by `build_cross_crate_index`) determine `use_cache` per source crate by checking `workspace_members` — non-workspace deps use `true` (cache), workspace members use `false` (always regenerate). All callers share the same `target/doc/` directory — `.bin` cache files accumulate there and are never cleaned by `--clean`.
- `pre_warm_cross_crate_json` → `rustdoc_json::load_lockfile_packages`: Pre-warming validates candidate crate names against Cargo.lock. If Cargo.lock is absent (e.g. fresh checkout with workspace not yet resolved), `available_packages` is empty → all cross-crate pre-warming is silently skipped. Individual per-crate generation in `cross_crate.rs` still runs as fallback.

## Extension Points & Change Recipes
- **Add a new `--no-*` filter flag**: Touch `cli.rs` (`FilterArgs` struct), `render.rs` (`should_render_item`), all test helpers (`default_filter()` in each file). Compile errors guide you.
- **Add a new item type**: Touch `render.rs` (add renderer + visibility check), `test_fixture/src/lib.rs` (add example), `tests/integration.rs` (add assertion). Missing the visibility check → private items leak silently. For cross-crate rendering also touch `cross_crate.rs` (`AccessibleItemKind::from_item` match arm), `render.rs` (`render_virtual_tree`), `search.rs` (`search_cross_crate_index` kind mapping), and `summary.rs` (`summarize_cross_crate_index` kind mapping) — missing any one of these silently omits the new type from cross-crate output only.

## Common Mistakes
- Calling `render_item()` for a new item type without a preceding `is_visible_from()` check → private items appear in output.

## Technical Debt
- String-based glob detection/replacement in `apply_glob_expansions` — fragile (whitespace-normalized string matching). See `glob-expansion.md`.
- `--verbose` / `-v` prints progress to stderr; messages are in `lib.rs` only (not in utility modules).
- Cross-crate hop limit is hardcoded at 5 for targeted module resolution (`CrossCrateResolution` path) and at 8 for `build_cross_crate_index`'s `walk_accessible`. The two paths have different limits with no single constant controlling both — changing one does not change the other.
- `render_cross_crate_api` impl blocks render without nesting indent: `render_inlined_impl_blocks` uses `""` as the indent internally, so impl blocks for cross-crate types appear at top level in the output regardless of module nesting depth. The function doesn't accept an indent parameter — it's the 5th arg `source_type_name` which is a filter, not indent.
