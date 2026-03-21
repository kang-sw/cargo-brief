---
title: "Private module glob re-exports invisible to search/api"
status: done
started: 2026-03-21
completed: 2026-03-21
---

## Problem

`pub use private_mod::*` where `private_mod` is a private module — the dominant
pattern in Bevy — makes items invisible to both search and api pipelines when
the glob is inside a **nested** module (not the crate root).

```rust
// bevy_render/src/render_resource/mod.rs
mod bind_group;          // PRIVATE
pub use bind_group::*;   // re-exports AsBindGroup trait etc.
```

`cargo brief search bevy_render "AsBindGroup"` → misses the trait.

## Root Cause

Confirmed via rustdoc JSON inspection of `bevy_render`:

- `render_resource` module contains `pub use bind_group::*` (glob Use item)
- rustdoc JSON: `use_item.source = "bind_group"` (bare name, no parent path)
- `compute_reachable_set` → `walk_public` → `find_module_entry("bind_group")`
  tries `bevy_render::bind_group` and bare `"bind_group"` — neither exists
  (actual path: `bevy_render::render_resource::bind_group`)
- Glob silently skipped → all items inside unreachable

The v0.5.1 fix only worked for **root-level** private modules because
`find_module_entry("hidden_reexport")` resolves via `crate_name::hidden_reexport`.

Key insight: `walk_modules` (model construction) recurses into ALL modules
regardless of visibility — `module_index` DOES contain entries like
`bevy_render::render_resource::bind_group`. The fix was to pass the current
module path through `walk_public` to construct the fully-qualified path.

### Result (5d7c632) - 26-03-21

Fixed `walk_public` in `src/model.rs` to accept and propagate `module_path: &str`.
Glob Use items now resolve sources relative to the current module first, then
fall back to bare source (preserving root-level behavior).

**Changes:**
- `src/model.rs`: `walk_public` gains `module_path` parameter. Glob source
  resolution tries `"{module_path}::{source}"` first, then bare `"{source}"`.
  Module children propagate their path via `"{module_path}::{name}"`.
- `test_fixture/src/lib.rs`: Added `mod nested_private` with pub items inside
  `pub mod outer`, with `pub use nested_private::*`.
- `tests/integration.rs`: 2 new tests (search + api) for nested private glob.

All 148 integration tests pass (146 existing + 2 new).
