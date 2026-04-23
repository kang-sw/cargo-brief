---
domain: Glob Re-Export Expansion
description: String-based glob detection, Phase 1/2 inlining, coupling with render output
sources:
  - src/lib.rs
  - src/render.rs
related:
  - visibility.md
  - remote-pipeline.md
---

# Glob Re-Export Expansion

## Entry Points
- `src/lib.rs` — `expand_glob_reexports()` detects globs and named cross-crate re-exports, generates source crate JSON, and calls `collect_glob_items_recursive()`; `apply_glob_expansions()` replaces glob lines and named re-export lines in the output string.
- `src/render.rs` — `render_inlined_items()` renders Phase 2 full definitions; `render_single_inlined_item()` renders a single named item from source models.

## Module Contracts
- `expand_glob_reexports()` guarantees: (1) First pass — scans the target module's direct children for `Use` items with `is_glob=true`. For each glob, calls `collect_glob_items_recursive()` (max depth 8, cycle-safe via `visited: HashSet`) to follow cross-crate glob chains. (2) Second pass — scans the same children for non-glob `Use` items whose `id` is absent from the local index (cross-crate named re-exports); generates source crate JSON if not already present from pass 1 (shared `source_models` map). Returns `GlobExpansionResult` with three fields: `item_names` (glob sources only), `source_models` (shared by both glob and named; key=crate name), and `named_reexports` (named sources; key=crate name, value=Vec<(item_name, full_source_path)>). Errors during source crate JSON generation are silently skipped (`else { continue }`). Does NOT receive `use_cache` from `PipelineContext` directly — instead computes `dep_use_cache` per source by checking membership in `workspace_members`: non-members use `true` (cache), members use `false` (regenerate).
- `try_generate_rustdoc_json()` guarantees: tries the source name as-is first (Rust identifier, underscores), then retries with `_` → `-` substitution (Cargo package names use hyphens). Both failures → returns `None` silently with no log unless `verbose`.
- `apply_glob_expansions()` guarantees: Phase 2 glob loop iterates `item_names.keys()` (NOT `source_models.keys()`) so named-only source crates do not pollute `seen_names` before the glob items are rendered. Named expansion loop follows Phase 2, calling `render_single_inlined_item()` per item and replacing `pub use {full_source_path};` lines. Both phases share the same `seen_names` set for deduplication. Both phases are suppressed by `--no-expand-glob`.
- `render_inlined_items()` guarantees: renders with `observer=source_crate_name` and `same_crate=false` (hardcoded). Deduplicates across sources via `seen_names: HashSet`.
- `render_single_inlined_item()` guarantees: searches root-level public items in source models by name; follows `Use` chains one hop into the same model; returns `None` for modules (module re-exports are left as-is), for items suppressed by `--no-*` filters, for items already in `seen_names`, or if the item is not found.

## Coupling
- Render output format ↔ glob detection: `render_module_api()` MUST emit glob re-exports recognizable after whitespace normalization as `pub use {source}::*;`. Named re-exports must be emitted as `pub use {full::source::path};`. `replace_glob_lines()` normalizes by collapsing whitespace before comparing. Any change to either line format (extra tokens, different keyword order) → silent failure — the line is left unexpanded.
- `apply_glob_expansions()` Phase 2 iterates `item_names.keys()` for glob sources, NOT `source_models.keys()`. If this is changed to `source_models.keys()`, named-only source crates (present only for named re-exports) would be included in the glob Phase 2 loop, adding their items to `seen_names` prematurely → those items silently skipped in the named expansion loop.
- `document_private_items` in glob expansion: Always `false` in `try_generate_rustdoc_json()`, even for same-crate globs. Internal crate globs → `pub(crate)` items absent from source JSON → silently missing from expansion.
- `render_inlined_items` and `render_single_inlined_item` both call `should_render_item`, so `--no-*` filters ARE applied to Phase 2 inlined definitions and named re-export inlined definitions.
- `collect_glob_items_recursive()` skips `Module` items at root level — glob-re-exported submodules are not recursed into, only direct items and further `Use` items at the root.
- Named re-export of a module (e.g. `pub use serde::de;`): `render_single_inlined_item()` returns `None` → line is left as `pub use serde::de;` unchanged. This is intentional, not a bug.

## Extension Points & Change Recipes
- **Add a deeper glob chain to fixture**: Add a new workspace member to `test_fixture/Cargo.toml` under `[workspace] members`, add a dependency in the intermediate crate, and add `pub use new_crate::*;` in that crate's lib.rs. Without the workspace member entry, `cargo rustdoc -p` will not find the crate → silent skip.
- **Increase cross-crate glob depth limit**: Change `MAX_DEPTH` constant in `collect_glob_items_recursive()`.

## Common Mistakes
- Source crate uses a hyphenated package name (e.g. `glob-source`) but the rustdoc `use_item.source` field gives the Rust identifier form (`glob_source`). `try_generate_rustdoc_json()` handles this, but only for direct `_` → `-` substitution. Crates with mixed naming patterns not covered by this transform → silent skip.
- Source crate JSON generation failure (e.g., source not a workspace member) → `continue` silently. User sees unexpanded `pub use source::*;` or `pub use source::Item;` with no indication why.
- Adding a glob re-export chain deeper than 8 hops → items beyond depth 8 silently absent from expansion.
- `collect_glob_items_recursive()` adds `nested_model` to `all_models` AFTER the recursive call returns (post-order). Phase 2 rendering iterates models in post-order — leaf models appear before their parents in the Vec.
- Named re-export where `use_item.id` is `None` (primitive re-exports, unresolvable items) → silently skipped in the second pass. The `pub use` line is left unexpanded.
- Named re-export of a re-exported item that itself points to a foreign id not in the source model's index → `render_single_inlined_item()` finds no matching child and returns `None` → line left unexpanded silently.

## Technical Debt
- String-based glob detection is fragile. A marker-based or AST-aware approach would be more robust.
- Phase 1 and Phase 2 data are always both generated regardless of `--no-expand-glob` flag. Minor performance cost.
- Phase 2 inlining follows re-export targets to render actual definitions, which means the rendered item type may differ from the Use item that triggered it.
- No logging/warning when glob expansion silently skips a source crate (only depth-based recursion logs at `--verbose`).
- `render_single_inlined_item()` only searches root-level items of the source crate. Named re-exports of items in nested modules (e.g. `pub use serde::de::Deserialize;`) will not be found and the line is left unexpanded.
- Named expansion cannot follow multi-hop glob chains: the source models for named re-exports are a single `Vec<CrateModel>` with no recursive discovery — only `collect_glob_items_recursive()` fills recursive hop models, and that runs only for glob Use items.
