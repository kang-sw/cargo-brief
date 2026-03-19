---
title: "search finds 0 results for re-exported items from pub(crate) modules via glob"
status: done
started: 2026-03-20
completed: 2026-03-20
---

## Problem

`cargo brief search` returns 0 results for items like `Material`, `MaterialPlugin`,
`MeshMaterial3d` from `bevy_pbr`, even though these are re-exported through `pub use`
globs at the crate root and consumed via the `bevy` facade crate.

## Reproduction

```
cargo brief search --crates bevy Material
```

Expected: finds `bevy_pbr::Material` trait, `MaterialPlugin`, etc.
Actual: finds only items with "Material" in their name from *public* modules
(`diagnostic::`, `wireframe::`), misses the core types entirely.

## Root Cause (verified)

Two paths to discovering these items are both blocked:

### Path 1: Direct module traversal

- `bevy_pbr::material` module has **`visibility: crate`** (pub(crate))
- `compute_reachable_set` in `model.rs` only follows `Visibility::Public` children
- Therefore the `material` module is NOT in the reachable set
- `walk_module` in `search.rs` skips unreachable children → never enters `material`

### Path 2: Glob re-export

- `bevy_pbr` root has `pub use material::*` (glob re-export)
- `walk_module` explicitly skips glob Use items: `ItemEnum::Use(use_item) if !use_item.is_glob`
- Glob uses fall through to `_ => {}` catch-all

**Net result**: pub items inside pub(crate) modules that are glob-re-exported at root
level are invisible to both the search walker and the reachable set.

This is the standard Rust pattern: define items in a private/pub(crate) module,
re-export publicly via glob. Very common in bevy, axum, tokio, etc.

## Verified data (bevy_pbr 0.18.1 rustdoc JSON)

```
material module: visibility = crate, children = 62
  Material trait: visibility = public  ← invisible to search
  MaterialPlugin struct: visibility = public  ← invisible to search
  MaterialPipeline struct: visibility = public  ← invisible to search

Root glob re-exports:
  pub use material::*   (is_glob = true)  ← skipped by walker
  pub use mesh_material::*
  pub use pbr_material::*
  pub use extended_material::*
```

## Impact

Major usability gap. The "pub(crate) module + glob re-export" pattern is the
dominant Rust crate organization style. Affects bevy, axum, tokio, serde, and
most non-trivial crates.

## Fix Direction

The reachable set computation (`compute_reachable_set`) needs to follow glob
re-exports: when a `pub use module::*` is encountered and `module` is in-crate,
mark all public items within that module as reachable (transitively).

This would fix both the search walker (items become reachable) and the API
renderer (same reachable set is used).

Alternative: the search walker could independently expand glob re-exports by
walking the source module's public items. But fixing the reachable set is more
correct since it affects all pipelines.

### Result - 26-03-20

Fixed `walk_public()` in `model.rs` to follow intra-crate glob re-exports.
When a `pub use module::*` is encountered, the source module is resolved via
`find_module_entry()` and its public items are walked into the reachable set.

Changes:
- `model.rs`: Added `find_module_entry()` helper, refactored `find_module()` to use it.
  Modified glob `Use` arm in `walk_public()` to resolve source module and recursively
  walk its public items. Cycle guard via `reachable.insert()` return value.
- `test_fixture/src/lib.rs`: Added `pub(crate) mod hidden_reexport` with `GlobTrait`
  and `GlobStruct`, glob-re-exported at crate root.
- `tests/integration.rs`: Added 3 tests exercising external-crate perspective
  (same_crate=false, reachable set) for search and API rendering of glob-re-exported items.

No changes to `search.rs` or `render.rs` — fix is contained in `model.rs`.
All 112 tests pass.
