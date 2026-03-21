# Glob Re-Export Expansion

## Entry Points
- `src/lib.rs` — `expand_glob_reexports()` detects globs, generates source crate JSON, and calls `collect_glob_items_recursive()`; `apply_glob_expansions()` replaces all matching glob lines in the output string.
- `src/render.rs` — `render_inlined_items()` renders Phase 2 full definitions.

## Module Contracts
- `expand_glob_reexports()` guarantees: scans the target module's direct children for `Use` items with `is_glob=true`. For each glob, calls `collect_glob_items_recursive()` (max depth 8, cycle-safe via `visited: HashSet`) to follow cross-crate glob chains. Returns `GlobExpansionResult` where `source_models: HashMap<String, Vec<CrateModel>>` — the Vec holds the direct source model at index 0, followed by any recursively discovered models. Errors during source crate JSON generation are silently skipped (`else { continue }`). Does NOT receive `use_cache` from `PipelineContext` directly — instead computes `dep_use_cache` per source by checking membership in `workspace_members`: non-members use `true` (cache), members use `false` (regenerate).
- `try_generate_rustdoc_json()` guarantees: tries the source name as-is first (Rust identifier, underscores), then retries with `_` → `-` substitution (Cargo package names use hyphens). Both failures → returns `None` silently with no log unless `verbose`.
- `apply_glob_expansions()` guarantees: replaces ALL occurrences of each glob line (loops until no match remains). Iterates over every `CrateModel` in each source's Vec for Phase 2 rendering.
- `render_inlined_items()` guarantees: renders with `observer=source_crate_name` and `same_crate=false` (hardcoded). Deduplicates across sources via `seen_names: HashSet`.

## Coupling
- Render output format ↔ glob detection: `render_module_api()` MUST emit glob re-exports recognizable after whitespace normalization as `pub use {source}::*;`. `replace_glob_lines()` normalizes by collapsing whitespace before comparing. Any change to the glob line format (extra tokens, different keyword order) → silent failure.
- `document_private_items` in glob expansion: Always `false` in `try_generate_rustdoc_json()`, even for same-crate globs. Internal crate globs → `pub(crate)` items absent from source JSON → silently missing from expansion.
- `render_inlined_items` calls `should_render_item`, so `--no-*` filters ARE applied to Phase 2 inlined definitions.
- `collect_glob_items_recursive()` skips `Module` items at root level — glob-re-exported submodules are not recursed into, only direct items and further `Use` items at the root.

## Extension Points & Change Recipes
- **Add a deeper glob chain to fixture**: Add a new workspace member to `test_fixture/Cargo.toml` under `[workspace] members`, add a dependency in the intermediate crate, and add `pub use new_crate::*;` in that crate's lib.rs. Without the workspace member entry, `cargo rustdoc -p` will not find the crate → silent skip.
- **Increase cross-crate glob depth limit**: Change `MAX_DEPTH` constant in `collect_glob_items_recursive()`.

## Common Mistakes
- Source crate uses a hyphenated package name (e.g. `glob-source`) but the rustdoc `use_item.source` field gives the Rust identifier form (`glob_source`). `try_generate_rustdoc_json()` handles this, but only for direct `_` → `-` substitution. Crates with mixed naming patterns not covered by this transform → silent skip.
- Source crate JSON generation failure (e.g., source not a workspace member) → `continue` silently. User sees unexpanded `pub use source::*;` with no indication why.
- Adding a glob re-export chain deeper than 8 hops → items beyond depth 8 silently absent from expansion.
- `collect_glob_items_recursive()` adds `nested_model` to `all_models` AFTER the recursive call returns (post-order). Phase 2 rendering iterates models in post-order — leaf models appear before their parents in the Vec.

## Technical Debt
- String-based glob detection is fragile. A marker-based or AST-aware approach would be more robust.
- Phase 1 and Phase 2 data are always both generated regardless of `--expand-glob` flag. Minor performance cost.
- Phase 2 inlining follows re-export targets to render actual definitions, which means the rendered item type may differ from the Use item that triggered it.
- No logging/warning when glob expansion silently skips a source crate (only depth-based recursion logs at `--verbose`).
