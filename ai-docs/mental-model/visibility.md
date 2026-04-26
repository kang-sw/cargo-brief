---
domain: Visibility Resolution
description: "is_visible_from() semantics, same_crate inference, observer normalization, walk_public reachability"
sources:
  - src/model.rs
  - src/lib.rs
  - src/render.rs
  - src/summary.rs
related:
  - target-resolution.md
---

# Visibility Resolution

## Entry Points
- `src/model.rs` — `is_visible_from()` is the single visibility decision function.
- `src/lib.rs` — `same_crate` inference logic (local pipeline only; remote always passes `false`).
- `src/render.rs` — observer module path normalization before `is_visible_from()` calls.

## Module Contracts
- `model::is_visible_from()` guarantees: given correct `observer_module_path` and `same_crate`, it returns whether the item would compile if `use`d from that position. All visibility decisions in the codebase must flow through this function.
- `lib.rs` guarantees: `same_crate` is computed once and threaded to all downstream consumers. It checks `obs == resolved.package_name || obs.replace('-', "_") == model.crate_name()` (direct equality OR hyphen-normalized).
- `render.rs` guarantees: every item emitted to output has passed an `is_visible_from()` check. This is enforced by convention, not by the type system.
- Feature-gate annotations: `render_attrs()` in `render.rs` emits `#[cfg(feature = "...")]` before items that rustdoc recorded as requiring a specific feature (via `CfgTrace(...)` in the raw attribute list). It reconstructs the attribute by calling `parse_cfg_attribute()` / `reconstruct_cfg_attr()` from `cfg_parse.rs`. The doc comment (if any) is emitted **before** the `#[cfg(...)]` line, matching Rust convention. The `--no-feature-gates` flag on `FilterArgs` suppresses all `CfgTrace`-derived annotations. Feature-gate annotations are a rendering concern only — they do not affect `is_visible_from()` or the items included in output.
- `summary::count_module_items()` guarantees: a pre-pass over module children builds `use_module_ids` — the `Id`s that are targets of visible non-glob `Use` items pointing to a `Module`. In the main child loop, the `Module` arm skips any module whose `Id` is in `use_module_ids`, preventing double-counting when the same module is both a direct `pub mod` child and a named `pub use` re-export target in the same parent scope. The named `Use`→`Module` branch recurses into the target module's children under the alias name; modules with zero visible items after recursion are suppressed from the output.

## Coupling
- `same_crate` (lib.rs:68) ↔ `document_private_items` (rustdoc_json call): These MUST be consistent. If `same_crate=true`, JSON must be generated with `document_private_items=true`, otherwise `pub(crate)` items are absent from JSON and silently hidden. Currently both local and remote pipelines pass `true` — the remote pipeline does so to expose internal re-export chains for cross-crate following. `same_crate` is always `false` in the remote pipeline regardless.
- `same_crate` (lib.rs) ↔ `render_module_api` (render.rs): The `same_crate` flag is passed as a plain `bool`. No type safety prevents passing the wrong value.
- Observer normalization (render.rs:47-55) does NOT normalize hyphens, but `same_crate` detection (lib.rs:68) DOES. Passing `--at-mod "my-crate::foo"` when the crate name is `my_crate` → observer path won't match, visibility filtering silently wrong.

## Extension Points & Change Recipes
- **Add a new `Visibility` variant** (from `rustdoc_types`): Update `is_visible_from()` match arms. Rust exhaustive matching forces this. Defaulting new variants to `false` silently hides items.
- **Change observer semantics**: Must update BOTH lib.rs (same_crate detection) AND render.rs (observer normalization). No single source of truth for "who is the observer."
- **Modify glob source resolution in `walk_public`** (`model.rs`): The glob `Use` arm tries `"{resolution_path}::{source}"` (relative to parent) before falling back to bare `"{source}"`. This order is load-bearing — reversing it breaks nested private-module glob re-exports like `mod bind_group; pub use bind_group::*;` inside a deeper module (the bare source resolves to the wrong path or not at all). `walk_public` now tracks two paths: `resolution_path` (actual module hierarchy for glob source resolution) and `canonical_path` (public path that stays unchanged when entering private modules via glob). Returns `ReachableInfo` with `glob_private_modules` and `glob_inlined` metadata for downstream render/search/summary.
- **Named Use→Module alias resolution in `walk_public`** (`model.rs`): When a non-glob `Use` item targets a `Module` and the module is newly added to the reachable set, `walk_public` recurses into that module's children under the alias path. The alias is `child.name.as_deref().unwrap_or(&use_item.name)` — `child.name` takes priority with no empty-string guard; `use_item.name` is the fallback when `child.name` is `None`. This same formula applies in `summary.rs::count_module_items`. Deviating from this formula (e.g., adding an `.filter(|s| !s.is_empty())` guard) can produce empty alias paths, silently dropping all children of the re-exported module from the reachable set.

## Common Mistakes
- Calling `is_visible_from()` with a non-qualified observer path (e.g., `"utils"` instead of `"crate_name::utils"`) → `is_ancestor_or_equal()` fails, restricted items silently hidden.
- Feature-gate annotations are emitted by `render_attrs()` regardless of whether the user passed `-F <feature>`. An item gated on `feature = "tokio"` will show `#[cfg(feature = "tokio")]` in output even when `tokio` is enabled — this is informational annotation, not a visibility filter. Do not confuse with `is_visible_from()` filtering.
- Resolving a glob source name in `walk_public` using only the bare source (e.g., `find_module_entry("bind_group")`) without prefixing the current `module_path` → lookup misses modules that exist only as children of a private parent; glob silently skipped, all items inside unreachable.
- Omitting the `use_module_ids` pre-pass in `count_module_items` when processing a scope that contains both `pub mod submod` and `pub use parent::submod` → the module's items are counted twice in summary output, inflating item counts.
- Named `pub use private_parent::submod` where `submod` contains no public items after visibility filtering: the module is silently suppressed from summary output (empty-module suppression). Do not expect a zero-item module entry in summary output.
- Setting `same_crate=true` without generating JSON with `--document-private-items` → `pub(crate)` items absent from JSON, silently filtered.
- Glob expansion hardcodes `same_crate=false` and observer=source crate name (render.rs:93, 137). Inlined items are always filtered as cross-crate, even when the facade crate is the same crate. This is correct for external crates but wrong if applied to same-workspace globs.

## Technical Debt
- Hyphen/underscore normalization is inconsistent: lib.rs normalizes for `same_crate`, render.rs does not normalize observer paths. Could cause silent visibility errors with hyphenated crate names and `--at-mod`.
- No validation that the observer module path actually exists in the crate's module tree. Passing a non-existent observer → `is_ancestor_or_equal()` always returns false for restricted items.
- `render.rs` has 6 independent `is_visible_from()` call sites with no common dispatch. Missing one when adding a new rendering path → private items leak.
- Trait impl items skip `is_visible_from()` (render.rs:557-591), while inherent impl items check it (render.rs:593-619). Inconsistent but currently correct because trait impls only render associated types/consts.
