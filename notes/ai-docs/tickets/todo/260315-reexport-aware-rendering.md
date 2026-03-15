# Re-export-aware Rendering for External Crates

## Goal

When rendering an external crate (especially via `--crates`), show only items
reachable through the public API, but preserve the module structure they're
defined in. Currently `same_crate=true` is used as a workaround, which
over-exposes `pub(crate)` items.

## Problem

Facade crates like `hecs` define all types in private modules (`mod archetype;`)
and re-export them at root (`pub use archetype::Archetype;`). With
`document_private_items=false`, private modules are absent from the JSON and only
`pub use` lines appear. With `document_private_items=true` + `same_crate=true`
(current workaround), `pub(crate)` items that shouldn't be externally visible
leak through.

The desired behavior: generate JSON with full dump, but only render items that
are **reachable from the crate's public API**.

## Design

### Reachability Walk

Before rendering, compute a set of "publicly reachable" item IDs by walking
from the root module:

1. Start from root module's children
2. For each child:
   - `pub mod M` → mark M reachable, recurse into M
   - `pub use source::Item` → mark the **target** item reachable
   - `pub use source::*` → mark all public children of source reachable
   - `pub struct/enum/trait/fn/...` → mark reachable
   - `pub(crate) mod`, `mod` (private) → **skip** (not reachable externally)
3. For reachable structs/enums/unions, also mark their impl blocks reachable

Result: `HashSet<Id>` of all publicly reachable items.

### Rendering

Use `document_private_items=true` (so private modules exist in JSON) and
`same_crate=false` (normal external view). Two changes to `render_module_contents`:

1. **Module gate:** When encountering `ItemEnum::Module`, only recurse if the
   module ID is in the reachable set. This hides private modules while showing
   `pub mod` with full contents.

2. **Use inlining:** When encountering `pub use source::Item` where the source
   module is NOT reachable (i.e., private module), follow the target and render
   the actual definition inline (like `render_inlined_items` does). When the
   source module IS reachable (i.e., `pub mod`), keep the `pub use` line as-is
   since the user can navigate to that module.

### Scope

This applies to:
- `run_remote_pipeline` (`--crates` path) — always
- `run_pipeline` with `same_crate=false` — when target crate differs from observer

For `same_crate=true` (inspecting own crate), current behavior is correct.

### Revert Required

Revert the `same_crate=true` workaround in `run_pipeline` (lib.rs) and
`run_remote_pipeline`. Restore the commented-out `observer_crate` logic.
Keep `document_private_items=true`.

## Files to Modify

| File | Changes |
|------|---------|
| `src/model.rs` or `src/render.rs` | `compute_reachable_set(model) -> HashSet<Id>` |
| `src/render.rs` | `render_module_contents`: module gate + use inlining |
| `src/lib.rs` | Restore `observer_crate`/`same_crate` logic (currently commented out), pass reachable set |
| `src/search.rs` | Apply reachable filter to search walker |
| `tests/` | Regression test with pinned facade crate |

## Post-implementation Checklist

After reachability walk is implemented, **un-ignore** the 12 cross-crate
visibility tests that were disabled by the `same_crate=true` hotfix:

- `tests/external_crate_integration.rs` (4 tests):
  `either_into_either_trait`, `either_iter_either_struct`,
  `either_hides_pub_crate_modules`, `either_depth_zero_still_shows_root_items`
- `tests/subprocess_integration.rs` (3 tests):
  `auto_visibility_cross_crate`, `auto_visibility_reverse`,
  `at_package_cross_crate`
- `tests/workspace_integration.rs` (5 tests):
  `core_lib_external_view_hides_pub_crate_items`,
  `core_lib_external_view_hides_crate_method`,
  `core_lib_external_view_struct_has_hidden_field_indicator`,
  `app_external_view`, `core_lib_utils_external_hides_crate_items`

All are tagged `#[ignore = "TODO(260315): restore after reexport-aware reachability walk"]`.

## Integration Tests

Add `tests/remote_facade_integration.rs` with `hecs@0.11.0` pinned as the
regression fixture. Tests are `#[ignore = "network"]` like the existing
remote crate tests.

**Structural assertions (not full content equality):**
- Root has `mod archetype {`, `mod entities {`, etc. (private modules rendered
  because their items are reachable via `pub use`)
- `pub struct Archetype` appears inside `mod archetype { ... }`
- `pub struct World` appears inside `mod world { ... }`
- `pub struct Entity` appears inside `mod entities { ... }`
- No `pub(crate)` items in output (grep for `pub(crate)` → 0 matches)
- `pub use` re-export lines still present at root (pointing to rendered modules)
- Total module count ≥ 10 (hecs 0.11 has 13 internal modules)
- Output is non-trivially long (> 100 lines, proving definitions are rendered)

**either@1.15 unchanged:**
- `pub enum Either` with variants still appears
- Module structure preserved

**Search mode:**
- `--crates hecs@0.11.0 --search Archetype`: finds struct + methods, no
  `pub(crate)` items in results
