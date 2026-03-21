---
title: "Show canonical public paths for glob-reexported items"
status: idea
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
path, not the private source module's path. This affects both search and api
pipelines.

### Root Cause (code-traced)

Three interacting issues:

1. **Reachable set** (`model.rs` `walk_public`): Inserts the private module
   itself as reachable (lines 248, 252) → API pipeline renders it as
   `mod bind_group { /* ... */ }`.

2. **Search paths** (`search.rs` `walk_module`): Builds paths by physical
   `{parent_path}::{name}` accumulation (line 493-497) → includes private
   module name in display path.

3. **Glob expansion** (`lib.rs` `expand_glob_reexports`): String-based
   post-processing treats bare module names as crate names → fails silently
   for intra-crate private modules.

### Design: `ReachableInfo` struct

Replace `compute_reachable_set() -> HashSet<Id>` with a richer return type:

```rust
pub struct ReachableInfo {
    /// All items reachable through the public API.
    pub reachable: HashSet<Id>,

    /// Item ID → canonical public path.
    /// For items reached via glob re-exports from private modules,
    /// maps to the re-exporting (public) module's path.
    /// Items NOT in this map use CrateModel::item_module_path.
    pub canonical_paths: HashMap<Id, String>,

    /// Private module IDs reachable ONLY because of glob re-exports.
    /// API render should NOT render these as `mod name { ... }`.
    pub glob_private_modules: HashSet<Id>,
}
```

`walk_public` changes — track two paths:
- **resolution_path**: actual module path, for resolving relative glob sources
- **canonical_path**: the public module that glob-re-exports this private module

When `walk_public` enters a private module via `pub use private::*`:
- `resolution_path` = `"{parent}::{private_mod}"` (for nested glob resolution)
- `canonical_path` = `"{parent}"` (the public module doing the re-export)
- Items inside get `canonical_paths[item_id] = canonical_path`
- The private module ID goes into `glob_private_modules`

### Pipeline changes

**Search** (`search.rs`): `walk_module` checks `canonical_paths` for each item.
If present, use that as the display path prefix instead of physical parent path.

**API render** (`render.rs`): `render_module_contents` skips modules in
`glob_private_modules`. For the corresponding `pub use private::*` Use item,
instead of emitting the `pub use` line, directly render the private module's
public items at the current indentation level. This replaces the fragile
string-based `expand_glob_reexports` post-processing for intra-crate globs.

**Glob expansion** (`lib.rs`): `expand_glob_reexports` remains for cross-crate
globs only (where the source IS a separate crate). Intra-crate globs are now
handled at the render level.

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
