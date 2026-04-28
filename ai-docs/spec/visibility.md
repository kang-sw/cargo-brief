---
title: Visibility Semantics
summary: How cargo-brief determines which items to show based on the observer's position — covering observer setup, visibility levels, re-export interaction, and canonical path selection for facade crates.
---

# Visibility Semantics

Every item cargo-brief renders is evaluated against an **observer position** — the module from which the caller conceptually reads the API. Items that would not be accessible to `use` statements from that position are hidden. This spec describes how the observer is determined and how each Rust visibility level maps to a show/hide decision.

## Observer Position {#260423-observer-position}

The observer position consists of two components:

- **Package** — which crate is making the observation. Controls whether `pub(crate)` items are accessible.
- **Module** — which module path within the package. Controls whether `pub(super)` and `pub(in path)` items are accessible.

These are set via `--at-package` and `--at-mod`. Both flags apply to `api`, `summary`, and `search` subcommands only.

## Same-Crate Auto-Detection {#260423-same-crate-detection}

Before evaluating visibility, cargo-brief determines whether the observer is in the same crate as the target. The determination uses a three-step priority chain:

1. **Explicit `--at-package`** — if provided, the observer package is set to that value. `same_crate` is true when this package matches the target crate name (hyphen/underscore equivalent).
2. **cwd inference** — if the working directory matches a workspace member's manifest directory, that package is the observer. `same_crate` is set accordingly.
3. **Default** — if neither of the above resolves, the observer is external. `same_crate` is false.

### Effect of same_crate {#260423-same-crate-effects}

| `same_crate` | `pub(crate)` items | `--at-mod` honored |
|---|---|---|
| true | Visible | Yes |
| false | Hidden | No (silently ignored) |

## The `--at-mod` Flag {#260423-at-mod-flag}

Sets the observer's module path within the target crate. Uses `::` as separator. The crate name prefix may be omitted — `render::pass` and `my_crate::render::pass` are equivalent when `my_crate` is the target.

When `--at-mod` is omitted in same-crate mode, the observer defaults to the crate root.

`--at-mod` has no effect in cross-crate mode (`same_crate = false`). It is accepted but silently ignored.

What `--at-mod` controls in same-crate mode:

- **`pub(super)`** — visible when the observer is within the parent module's subtree.
- **`pub(in path)`** — visible when the observer path is a descendant-or-equal of the restricted path.
- **`pub(crate)`** — visible regardless of the specific module path (same-crate only).
- **`pub`** — always visible.

> [!note] Implementation Gap · 2026-04-23
> `--at-mod` is not propagated into the cross-crate facade rendering path (`render_virtual_tree`). When rendering a facade crate, the observer is fixed to the source crate root with `same_crate = false`. Specifying `--at-mod` on a facade crate target has no visible effect.

## The `--at-package` Flag {#260423-at-package-flag}

Three common uses:

- **Virtual workspace root** — when running from a workspace root with no package in cwd, `self` cannot be inferred. `--at-package <name>` selects the observer package explicitly.
- **Dependency perspective** — view a dependency as it appears from a specific package (e.g. a package that enables extra features or uses a patched version).
- **Force cross-crate** — specifying a package that does not match the target forces `same_crate = false` even when cwd would otherwise resolve to the target.

## Visibility Levels {#260423-visibility-levels}

Rust has five visibility forms that cargo-brief evaluates in order:

### `pub`

Always visible. Shown to both internal and external observers.

### `pub(crate)`

Visible only when `same_crate = true`. Hidden from all external observers.

### `pub(super)` and `pub(in path)`

Rustdoc JSON encodes `pub(super)` as `Restricted { parent: <module_id>, path: "super" }` and `pub(in path)` as `Restricted { parent: <module_id>, path: "<full_path>" }`.

Visible only in same-crate mode, and only when the observer module path is a descendant-or-equal of the restricted parent module.

> [!note] Implementation Gap · 2026-04-23
> `pub(super)` items are formatted in output as `pub(in super)` rather than the canonical Rust spelling `pub(super)`. This is a cosmetic inconsistency; the visibility filtering logic is correct. The `path` string from rustdoc JSON is forwarded verbatim to `format_visibility`.

### Private (Default visibility)

Items with no visibility keyword carry `Visibility::Default` in rustdoc JSON. These are treated as private and hidden. One exception applies: **impl blocks and their methods are always rendered**, regardless of their `Default` visibility marker. {#260423-default-visibility-impl-bypass}

This exception exists because rustdoc JSON assigns `Default` visibility to impl blocks as a structural convention, not as a true access-level signal. Impl block items inherit effective visibility from their parent type — if the type is visible, its impl items are shown.

### Struct and Union Fields {#260423-field-visibility}

Fields are individually visibility-checked. Fields that do not pass the visibility gate are suppressed; a `// .. private fields` placeholder is inserted when at least one field is hidden.

Enum variants have no independent visibility keyword — they are always shown when the enum itself is visible.

## Cross-Crate View {#260423-cross-crate-view}

When the observer is external (`same_crate = false`), cargo-brief builds a **reachable set** by walking the crate from its root and collecting every item reachable via `Visibility::Public` edges only. The walk includes:

- Public items in public modules.
- Items made public via `pub use` (named and glob re-exports). Their target items and all ancestor modules along the path are marked reachable.
- Impl blocks whose target type is reachable.

Output gates use the reachable set: an item not in the set is hidden from the rendered output. Error suggestion lists (e.g. for a not-found module or leaf item) are also filtered through the reachable set to prevent leaking private item names.

Local workspace crates receive the same cross-crate discovery treatment as remote crates — facade expansion and canonical-path resolution run for both.

## Named Re-Export Visibility {#260423-named-reexport-visibility}

A `pub use source::Name;` statement makes `Name` accessible from the re-exporting module. The reachability walk marks the target item and all its ancestor modules as reachable. This enables items in otherwise-private modules to surface at their re-exported path.

## Glob Re-Export Inlining {#260423-glob-reexport-inlining}

`pub use inner::*` with `inner` being a private module triggers **inlining**: the private module block is suppressed from output, and its items appear directly at the parent module level as if defined there.

The mechanism:

1. `compute_reachable_set` records the glob in `glob_inlined` and marks `inner` in `glob_private_modules`.
2. The render loop skips the private module block.
3. In place of the `pub use inner::*;` line, `render_inline_children` emits `inner`'s items at the current indent level.

The private module segment does not appear in the canonical path of the inlined items — items inherit the re-exporting module's path.

When a glob targets a **public** module, no inlining occurs: the public module renders normally and its items appear nested inside it.

When a glob targets an **external crate** module, the walk cannot follow into the external module's internals; the `pub use source::*;` line is emitted verbatim. The `CrossCrateIndex` pipeline handles deeper expansion for known facade crates.

Glob chains are followed recursively up to depth 8 with cycle detection. {#260423-cross-crate-depth-guard}

> [!note] Implementation Gap · 2026-04-23
> `pub(crate)` items from a re-exported source crate are suppressed during inlining. The inlining renderer hardcodes `same_crate = false` when expanding items from an external model, which hides `pub(crate)` items even in contexts where the source crate is the same as the target. This affects cross-crate named and glob re-export expansion only; intra-crate glob inlining is unaffected.

## Canonical Path Selection {#260423-canonical-path-selection}

When an item is reachable via multiple re-export paths (as is common in facade crates like bevy or axum), cargo-brief selects the **canonical path** for display using these rules:

1. **Prelude paths are deprioritized** — non-prelude paths win regardless of length.
2. **Shortest path wins** — among non-prelude paths, the shortest accessible path is selected.

The canonical path is what appears in `api`, `search`, and `summary` output. For example, `bevy::render::render_resource::AsBindGroup` rather than `bevy_render::render_resource::bind_group::AsBindGroup`.

This canonicalization is built by `CrossCrateIndex` / `build_cross_crate_index`, which walks the facade crate's public API top-down and tracks paths through both glob and named re-exports. Each item is deduplicated by `(source_crate, item_id)` pair.

## Visibility by Item Kind

| Item kind | Visibility rule |
|---|---|
| Module | Checked against observer. Glob-private modules are suppressed entirely; their items are inlined at the parent. |
| Struct / Enum / Union | Type checked first. Struct and union fields checked individually. Enum variants have no independent visibility. |
| Trait | Checked against observer. Associated items inherit the trait's visibility. |
| Fn / Const / Static / Type alias | Each item individually checked. |
| Impl block | Always rendered when parent type is visible (Default-visibility bypass). Methods individually checked only for `pub` gate (field access). |
| `pub use` (named) | Shown if reachable (cross-crate) or visible (same-crate). In default mode, replaced by the inlined full definition of the target item. |
| `pub use` (glob, private source) | Suppressed; source items inlined at parent level. |
| `pub use` (glob, public/external source) | Shown as `pub use source::*;` or rendered via `CrossCrateIndex`. |
