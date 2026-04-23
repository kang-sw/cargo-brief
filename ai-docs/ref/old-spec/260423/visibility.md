---
title: Visibility Semantics
summary: >
  How cargo-brief determines which items to show based on the observer's
  position.  Covers --at-mod, --at-package, same-crate inference, cross-crate
  filtering, visibility levels, re-export interaction, and glob inlining.

features:
  - Observer Position
  - Same-Crate Auto-Detection
    - Effect of same_crate
  - The `--at-mod` Flag
    - What --at-mod changes
  - The `--at-package` Flag
  - Visibility Levels
    - `pub`
    - `pub(crate)`
    - `pub(super)`
    - `pub(in path)`
    - Default (no visibility keyword)
  - Cross-Crate View
  - How Re-Exports Affect Visibility
    - Named Re-Exports
    - Glob Re-Exports
  - Glob Re-Export Inlining
    - Example
  - Visibility by Item Kind
    - Modules
    - Structs, Enums, Unions
    - Traits
    - Functions, Constants, Statics, Type Aliases
    - Impl Blocks
    - Re-Exports (use items)
---

# Visibility Semantics

cargo-brief's core differentiator is **visibility-aware output**: rather than
dumping every `pub` item, it shows only the items that would compile if `use`d
from the observer's position. This mirrors the Rust compiler's own visibility
rules and gives users an accurate picture of what they can actually reach.

The guiding invariant: **if `use <path>` would not compile from the observer's
module, the item does not appear in the output.**


## Observer Position

Every cargo-brief invocation has an implicit or explicit **observer** -- the
module position from which visibility is evaluated. Two flags control this:

- **`--at-package <name>`** -- which package the observer is in.
- **`--at-mod <path>`** -- which module within the target crate the observer
  occupies (e.g., `outer::inner`).

These flags only apply to the `api` and `summary` subcommands (which render
module-level views) and the `search` subcommand.

When neither flag is given, the observer defaults to an external caller looking
at the crate's public API surface (cross-crate view). When the tool detects the
observer is inside the same crate, it automatically switches to same-crate mode
and shows `pub(crate)` and restricted-visibility items as appropriate.


## Same-Crate Auto-Detection

cargo-brief infers whether the observer is inside the target crate by comparing
the **observer package** with the **target package**:

1. If `--at-package` is provided, its value is the observer package.
2. Otherwise, `cargo metadata` identifies which workspace package's manifest
   directory matches the current working directory. If found, that package
   name becomes the observer package.
3. If no package matches the cwd (e.g., running from a virtual workspace root
   or outside any package), the observer package is unset and the view defaults
   to cross-crate.

The tool then compares the observer package name with the target crate name.
Comparison accounts for Rust's hyphen-underscore equivalence (`my-crate` ==
`my_crate`). If they match, `same_crate` is true.

### Effect of same_crate

| same_crate | Behavior |
|------------|----------|
| `true`     | `pub(crate)` items visible. `--at-mod` is honored. Restricted visibility (`pub(super)`, `pub(in path)`) evaluated against observer module. |
| `false`    | Only `pub` items visible. `--at-mod` has no effect. Output filtered by reachability from the crate root. |


## The `--at-mod` Flag

`--at-mod` sets the observer's module path within the target crate. It is only
meaningful when `same_crate` is true (i.e., the observer is inside the target
crate). When the view is cross-crate, `--at-mod` is silently ignored.

The path should be relative to the crate root, using `::` separators:

```
cargo brief api self --at-mod utils::helpers
```

This tells the tool: "show me the API as it looks from `my_crate::utils::helpers`."

### What --at-mod changes

- **`pub(super)` items**: visible only if the observer is a direct child of the
  item's parent module. For example, a `pub(super)` function in `foo::bar` is
  visible when `--at-mod` is `foo::bar` or `foo::bar::baz`, but not when it is
  `foo::qux`.

- **`pub(in path)` items**: visible only if the observer is within the scope
  named by `path`. For example, `pub(in crate::foo)` is visible when the
  observer is `my_crate::foo` or any descendant, but not from `my_crate::bar`.

- **`pub(crate)` items**: always visible in same-crate mode regardless of
  `--at-mod`.

- **`pub` items**: always visible regardless of `--at-mod`.

When `--at-mod` is omitted in same-crate mode, the observer defaults to the
crate root. This means `pub(super)` and `pub(in path)` items are evaluated
from the root perspective.


## The `--at-package` Flag

`--at-package` overrides the auto-detected observer package. Use cases:

- **Running from a virtual workspace root** where no package is auto-detected:
  `--at-package my-crate` forces same-crate mode.
- **Viewing a dependency as if from a specific package**: useful in workspaces
  where one package depends on another and you want the same-crate perspective.
- **Forcing cross-crate mode**: `--at-package some-other-crate` ensures the
  view is external even if you happen to be inside the target crate's directory.


## Visibility Levels

Rust has five visibility levels. cargo-brief maps each to a filtering decision:

### `pub`

Always visible. Shown in both same-crate and cross-crate views.

### `pub(crate)`

Visible only in same-crate mode. Hidden from external observers. This is the
most common restricted visibility in practice.

### `pub(super)`

A shorthand for `pub(in <parent_module>)`. Visible if the observer module is
within the parent module's subtree. Only evaluated in same-crate mode; always
hidden cross-crate.

### `pub(in path)`

Visible if the observer module is an ancestor-or-equal to, or a descendant of,
the module named by `path`. Only evaluated in same-crate mode.

The check is: does the `path` module's fully-qualified name form a prefix of the
observer's fully-qualified module path? If yes, the observer is "inside" the
restricted scope and the item is visible.

### Default (no visibility keyword)

Items with no explicit visibility are private. cargo-brief hides them entirely.

**Exception -- impl items**: methods and associated items in `impl` blocks have
default visibility in rustdoc JSON. Their visibility is delegated to the parent
type: if the type is visible, its inherent impl methods are shown. Trait impl
items follow the trait's visibility.


## Cross-Crate View

When `same_crate` is false, cargo-brief uses **reachability-based filtering**
rather than per-item visibility checks. It computes a `ReachableInfo` set by
walking from the crate root and following only `pub` items.

This means the output for cross-crate views:

- Shows all items reachable through public module paths
- Includes items re-exported via `pub use` chains (even if the original
  definition lives in a private module)
- Hides `pub(crate)`, `pub(super)`, `pub(in path)`, and private items
- Shows items in private modules that are reachable through glob re-exports
  (see Glob Re-Export Inlining below)

The reachability walk also marks impl blocks of reachable types as reachable, so
methods on a public struct are included even though impl blocks themselves have
default visibility.


## How Re-Exports Affect Visibility

Re-exports (`pub use`) are the primary mechanism by which items from private or
nested modules become part of a crate's public API.

### Named Re-Exports

A `pub use inner::Foo;` in a public module makes `Foo` accessible from that
module regardless of `inner`'s visibility. cargo-brief:

- Shows the re-export as a `pub use` line in Phase 1 (default rendering)
- In Phase 2 (with `--no-expand-glob` disabled, the default), inlines the
  full definition at the re-export site, replacing the `pub use` line with
  the actual struct/enum/trait definition
- Annotates re-export lines with kind comments (`// struct`, `// trait`, etc.)

### Glob Re-Exports

A `pub use inner::*;` re-exports all public items from `inner`. This is
commonly used in facade crates (e.g., `bevy`) where the public module structure
differs from the internal organization.

cargo-brief follows glob chains up to depth 8 with cycle detection. When a glob
re-exports from a private module, the private module's items appear inlined at
the re-export site.


## Glob Re-Export Inlining

When a public module contains `pub use private_mod::*;` and `private_mod` is
not itself public, cargo-brief applies **glob inlining**:

- The private module is **not** rendered as a `mod private_mod { ... }` block
- Instead, all public items from the private module appear directly in the
  parent module that contains the glob re-export
- The `pub use private_mod::*;` line itself is suppressed
- The items appear as if they were defined directly in the parent module

This is tracked via `ReachableInfo`:

- `glob_private_modules` -- the set of private modules reached only through glob
  re-exports. The renderer skips these as module blocks.
- `glob_inlined` -- maps each glob `pub use` item to the private module it
  inlines. The renderer replaces the `use` line with the module's contents.

The behavior can be suppressed with `--no-expand-glob`, which reverts to showing
the raw `pub use path::*;` lines without inlining.

### Example

Given this internal structure:

```rust
// src/lib.rs
mod internal {
    pub struct Widget { pub name: String }
    pub fn create_widget() -> Widget;
}
pub use internal::*;
```

The cross-crate output is:

```rust
// crate my_crate
pub struct Widget {
    pub name: String,
}
pub fn create_widget() -> Widget;
```

The private `internal` module does not appear. Its items surface at the crate
root where the glob re-export lives.


## Visibility by Item Kind

### Modules

Modules are rendered as `mod name { ... }` blocks when within the recursion
depth, or `mod name { /* ... */ }` stubs when at the depth limit. Visibility
filtering applies to the module itself -- a `pub(crate)` module and all its
contents are hidden in cross-crate view.

Glob-private modules (private modules reached only through `pub use mod::*`)
are a special case: the module block is suppressed and its contents are inlined
at the parent.

### Structs, Enums, Unions

The type itself is visibility-checked. If visible:

- **Struct fields**: each field is individually visibility-checked. Hidden fields
  are replaced with `..` to indicate their presence without exposing details.
- **Enum variants**: always shown if the enum is visible (variants have no
  independent visibility in Rust).
- **Union fields**: treated like struct fields.

### Traits

Visible if the trait item passes the visibility check. All associated items
(methods, types, constants) within a visible trait are shown -- they inherit the
trait's visibility.

### Functions, Constants, Statics, Type Aliases

Each is individually visibility-checked. Shown if visible from the observer.

### Impl Blocks

Impl blocks in rustdoc JSON have default visibility. Their rendering depends on
the parent type:

- **Inherent impls**: methods and associated items are individually
  visibility-checked against the observer.
- **Trait impls**: if the trait and the type are both visible, the impl is shown.
  Simple trait impls (no associated items) are collapsed into per-type summary
  comments unless `--all` is passed.

### Re-Exports (use items)

`pub use` items are shown if they are reachable (cross-crate view) or visible
(same-crate view). In Phase 2 expansion (default), named re-exports are replaced
by the full definition; glob re-exports are replaced by inlined contents.
