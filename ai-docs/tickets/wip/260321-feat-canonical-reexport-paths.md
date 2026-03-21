---
title: "Show canonical public paths for glob-reexported items"
status: wip
started: 2026-03-21
---

## Problem

When items are accessed through `pub use private_mod::*` glob re-exports, the
output shows the **internal private module path** instead of the **canonical
public access path**.

```
# Current output:
fn render_resource::bind_group::AsBindGroup::as_bind_group(...)

# Expected (public access path):
fn render_resource::AsBindGroup::as_bind_group(...)
```

The `bind_group` module is private (`pub(super)`) — users can't import from
that path. The correct `use` path is `bevy_render::render_resource::AsBindGroup`.

## Phase 1: Intra-crate canonical paths

Items from `pub use private::*` should display at the re-exporting module's
path, not the private source module's path. This affects search, api, and
summary pipelines.

### Design: `ReachableInfo` struct

Replace `compute_reachable_set() -> HashSet<Id>` with:

```rust
pub struct ReachableInfo {
    pub reachable: HashSet<Id>,
    pub glob_private_modules: HashSet<Id>,
    pub glob_inlined: HashMap<Id, Id>,  // glob Use ID → private module ID
}
```

`walk_public` tracks dual paths:
- **resolution_path**: actual module path, for resolving relative glob sources
- **canonical_path**: stays at public parent when entering private modules

Pipeline changes:
- **render.rs**: skip modules in `glob_private_modules`, inline their contents
  via `render_module_contents` at the glob Use item
- **search.rs**: flatten path for glob-private modules (use parent_path)
- **summary.rs**: count items at parent level for glob-private modules

### Result (HEAD) - 26-03-21

Implemented as designed. Key deviations from the earlier draft:
- Dropped `canonical_paths: HashMap<Id, String>` in favor of `glob_inlined: HashMap<Id, Id>`.
  Pipelines derive paths from module position during traversal rather than
  pre-computing per-item canonical paths. Simpler and avoids storing redundant data.
- Render uses `render_module_contents` on the private module (not a new function)
  which inlines children without the `mod name { }` wrapper.

All 148 integration tests pass. Negative assertions added:
- Search: no `nested_private::` or `hidden_reexport::` in paths
- API: no `mod nested_private` or `mod hidden_reexport` wrappers
- Summary: `hidden_reexport` items counted at root level (existing test)

## Phase 2: Cross-crate canonical paths

Build a unified accessible-path index for facade crates (bevy, axum, etc.)
so all output paths reflect how users actually `use` items.

### Problem

Full re-export chain for AsBindGroup from bevy:

```
bevy (root)
  pub use bevy_internal::*;                    // glob
    bevy_internal (root)
      pub use bevy_render as render;           // rename!
        bevy_render (root)
          pub mod render_resource;
            pub use bind_group::*;             // glob, private module
              mod bind_group (pub(super))
                pub trait AsBindGroup
```

Current output shows `bevy_render::render_resource::bind_group::AsBindGroup`.
Expected: `bevy::render::render_resource::AsBindGroup`.

### Design: `CrossCrateIndex`

Two-level lookup: `accessible_path → (crate_idx, item_id) → Item`.

```rust
struct AccessibleEntry {
    accessible_path: String,       // "render::render_resource::AsBindGroup"
    crate_idx: usize,             // index into source_models
    item_id: Id,                  // ID within that crate's rustdoc JSON
}

struct CrossCrateIndex {
    source_models: Vec<CrateModel>,
    items: Vec<AccessibleEntry>,   // sorted by accessible_path
}
```

**Build algorithm**: Top-down recursive walk from facade root with prefix tracking.

```
walk(crate_model, prefix="", visited):
  for child in root.public_children:
    match child:
      pub use other_crate::*        → load other_crate, walk(other, prefix, visited)
      pub use other_crate as alias  → load other_crate, walk(other, prefix+"alias", visited)
      pub use private_mod::*        → walk into private_mod, keep prefix (Phase 1 pattern)
      pub mod name                  → walk(name, prefix+"name", visited)
      pub use specific::Item        → register(prefix+"Item", source_crate, item_id)
      leaf item                     → register(prefix+"name", current_crate, item_id)
```

**Trace through bevy example**:
```
bevy root, prefix=""
├─ pub use bevy_internal::*  → load bevy_internal, prefix=""
│  ├─ pub use bevy_render as render  → load bevy_render, prefix="render"
│  │  └─ pub mod render_resource  → prefix="render::render_resource"
│  │     └─ pub use bind_group::*  → private, prefix stays
│  │        └─ pub trait AsBindGroup
│  │           → register("render::render_resource::AsBindGroup", bevy_render_idx, id)
│  ├─ pub use bevy_math as math  → prefix="math"
│  │  └─ pub struct Vec3  → register("math::Vec3", bevy_math_idx, id)
│  └─ pub mod prelude  → prefix="prelude"
│     └─ pub use bevy_math::Vec3  → register("prelude::Vec3", bevy_math_idx, id)
```

**Dedup**: Group by `(crate_idx, item_id)`, keep shortest non-prelude path.

**Pipeline integration**:
- **Search**: match on `accessible_path`, fetch item via `source_models[crate_idx]`
- **API render**: split `accessible_path` by `::` → build virtual module tree
- **Summary**: count by first `accessible_path` segment

**Parallelization room**: `Vec<AccessibleEntry>` is owned data, no cross-crate
references. Each source crate's walk is independent → future `rayon` par_iter.

**Performance**: In-memory, no external index needed. ~50K items × ~50 bytes
≈ ~5MB for bevy. Bottleneck remains rustdoc JSON generation (already cached).

### Result (HEAD) - 26-03-21

Implemented as designed. All three pipelines (search, api, summary) now use
`CrossCrateIndex` instead of the old `discover_all_reexported_crates` + per-sub-crate
loop pattern.

Key structures:
- `AccessibleEntry { accessible_path, crate_idx, item_id, item_kind }` — one per
  accessible item, with the path reflecting user-facing `use` paths.
- `CrossCrateIndex { source_models: Vec<(CrateModel, ReachableInfo)>, items: Vec<AccessibleEntry> }`
- `build_cross_crate_index()` with recursive `walk_accessible()` (depth-limited to 8,
  cycle-safe via `visited_crates` set). Handles pub mod, intra-crate glob (private
  module flattening via Phase 1 pattern), cross-crate glob, cross-crate named
  reexport (with alias tracking), and leaf items.
- Dedup: group by `(crate_idx, item_id)`, keep shortest non-prelude path.

Pipeline integration:
- **Search**: `search_cross_crate_index()` matches against accessible paths, renders
  via existing `render_leaf()`. Impl/trait items walked with accessible path prefix.
- **API**: `render_cross_crate_api()` builds a `VirtualNode` tree from accessible
  paths, renders items inside nested `mod { }` wrappers.
- **Summary**: `summarize_cross_crate_index()` counts items per accessible path
  segment, replaces string-munging `merge_sub_crate_summary`.

Deviations from plan:
- `source_models` stores `(CrateModel, ReachableInfo)` tuple (not just `CrateModel`)
  for caching reachable sets.
- Used `unsafe` raw pointers (3 places) to split borrow between `source_models`
  and the walk's mutable borrow — safe because we only append to the Vec and
  existing indices remain stable.
- `AccessibleItemKind::Use` variant dropped — not needed since Use items are
  either resolved to their target kind or represent module-level structure.
- `discover_all_reexported_crates()`, `SubCrate`, and `resolve_single_reexport()`
  removed as dead code (no callers after Phase 2). Targeted module resolution
  uses `resolve_cross_crate_module()` + `CrossCrateResolution` (kept).

Test fixture: Added `pub use glob_inner as inner_alias;` to glob-source for rename
testing. 7 new integration tests, 156 total (was 149).
