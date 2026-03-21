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

## Phase 2: Cross-crate canonical paths (separate, harder)

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

Canonical path from bevy's perspective: `bevy::render::render_resource::AsBindGroup`

Challenges:
- Glob re-exports are 1:N (one glob → all public items) — reverse mapping is
  expensive
- Renames (`bevy_render as render`) require tracking name substitutions
- Chain crosses 3 crates (bevy → bevy_internal → bevy_render)
- Each crate has its own rustdoc JSON with separate ID spaces

This is a separate ticket when Phase 1 is stable.
