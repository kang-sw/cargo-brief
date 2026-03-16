# Visibility Resolution

## Entry Points
- `src/model.rs` — `is_visible_from()` is the single visibility decision function.
- `src/lib.rs:62-71` — `same_crate` inference logic.
- `src/render.rs:47-55` — observer module path normalization.

## Module Contracts
- `model::is_visible_from()` guarantees: given correct `observer_module_path` and `same_crate`, it returns whether the item would compile if `use`d from that position. All visibility decisions in the codebase must flow through this function.
- `lib.rs` guarantees: `same_crate` is computed once and threaded to all downstream consumers. It checks `obs == resolved.package_name || obs.replace('-', "_") == model.crate_name()` (direct equality OR hyphen-normalized).
- `render.rs` guarantees: every item emitted to output has passed an `is_visible_from()` check. This is enforced by convention, not by the type system.

## Coupling
- `same_crate` (lib.rs:68) ↔ `document_private_items` (rustdoc_json call): These MUST be consistent. If `same_crate=true`, JSON must be generated with `document_private_items=true`, otherwise `pub(crate)` items are absent from JSON and silently hidden. Currently enforced by: local pipeline always uses `true`, remote pipeline always uses `false`.
- `same_crate` (lib.rs) ↔ `render_module_api` (render.rs): The `same_crate` flag is passed as a plain `bool`. No type safety prevents passing the wrong value.
- Observer normalization (render.rs:47-55) does NOT normalize hyphens, but `same_crate` detection (lib.rs:68) DOES. Passing `--at-mod "my-crate::foo"` when the crate name is `my_crate` → observer path won't match, visibility filtering silently wrong.

## Extension Points & Change Recipes
- **Add a new `Visibility` variant** (from `rustdoc_types`): Update `is_visible_from()` match arms. Rust exhaustive matching forces this. Defaulting new variants to `false` silently hides items.
- **Change observer semantics**: Must update BOTH lib.rs (same_crate detection) AND render.rs (observer normalization). No single source of truth for "who is the observer."

## Common Mistakes
- Calling `is_visible_from()` with a non-qualified observer path (e.g., `"utils"` instead of `"crate_name::utils"`) → `is_ancestor_or_equal()` fails, restricted items silently hidden.
- Setting `same_crate=true` without generating JSON with `--document-private-items` → `pub(crate)` items absent from JSON, silently filtered.
- Glob expansion hardcodes `same_crate=false` and observer=source crate name (render.rs:93, 137). Inlined items are always filtered as cross-crate, even when the facade crate is the same crate. This is correct for external crates but wrong if applied to same-workspace globs.

## Technical Debt
- Hyphen/underscore normalization is inconsistent: lib.rs normalizes for `same_crate`, render.rs does not normalize observer paths. Could cause silent visibility errors with hyphenated crate names and `--at-mod`.
- No validation that the observer module path actually exists in the crate's module tree. Passing a non-existent observer → `is_ancestor_or_equal()` always returns false for restricted items.
- `render.rs` has 6 independent `is_visible_from()` call sites with no common dispatch. Missing one when adding a new rendering path → private items leak.
- Trait impl items skip `is_visible_from()` (render.rs:557-591), while inherent impl items check it (render.rs:593-619). Inconsistent but currently correct because trait impls only render associated types/consts.
