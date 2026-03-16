# Glob Re-Export Expansion

## Entry Points
- `src/lib.rs:176-255` — `expand_glob_reexports()` detects globs and generates source crate JSON.
- `src/lib.rs:145-169` — `apply_glob_expansions()` replaces glob lines in output string.
- `src/render.rs:82-165` — `render_inlined_items()` renders Phase 2 full definitions.

## Module Contracts
- `expand_glob_reexports()` guarantees: scans only the target module's direct children for `Use` items with `is_glob=true`. Returns `GlobExpansionResult` with both `item_names` (Phase 1) and `source_models` (Phase 2). Errors during source crate JSON generation are silently skipped (`else { continue }`).
- `apply_glob_expansions()` guarantees: replaces glob lines using exact string matching (`pub use {source}::*;\n`). Only the FIRST occurrence of each glob line is replaced.
- `render_inlined_items()` guarantees: renders with `observer=source_crate_name` and `same_crate=false` (hardcoded). Deduplicates across sources via `seen_names: HashSet`.

## Coupling
- Render output format ↔ glob detection: `render_module_api()` MUST emit glob re-exports as exactly `pub use {source}::*;\n`. `apply_glob_expansions()` searches for this exact pattern. Any formatting change (whitespace, comments, semicolons) → silent failure.
- `document_private_items` in glob expansion: Always `false` (lib.rs:214), even for same-crate globs. Internal crate globs → `pub(crate)` items absent from source JSON → silently missing from expansion.
- `render_inlined_items` calls `should_render_item` at lines 130 and 147, so `--no-*` filters ARE applied to Phase 2 inlined definitions.

## Extension Points & Change Recipes
- **Support globs in submodules** (not just target module): Change `expand_glob_reexports()` to walk recursively instead of checking only direct children. Must handle cycles.
- **Fix first-occurrence-only replacement**: Replace `output.find()` with position-aware iteration or marker-based approach.

## Common Mistakes
- Two modules defining `pub use same_source::*;` → only the first is expanded, second remains as literal `pub use` line. No warning.
- Source crate JSON generation failure (e.g., source not in workspace) → `continue` silently. User sees unexpanded `pub use source::*;` with no indication why.
- Adding a glob re-export to `test_fixture` without updating `tests/facade_crate_integration.rs` → no test coverage for the new glob.

## Technical Debt
- String-based glob detection is fragile. A marker-based or AST-aware approach would be more robust.
- Phase 1 and Phase 2 data are always both generated regardless of `--expand-glob` flag. Minor performance cost.
- Phase 2 inlining follows re-export targets to render actual definitions, which means the rendered item type may differ from the Use item that triggered it.
- No logging/warning when glob expansion silently skips a source crate.
