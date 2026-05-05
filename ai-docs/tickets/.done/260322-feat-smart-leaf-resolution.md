---
title: "Smart leaf item resolution in api target path"
status: done
started: 2026-03-22
completed: 2026-03-22
---

# Smart leaf item resolution in `api` target path

## Summary

When `cargo brief api <target> <path>` resolves a path whose final segment
is not a module, treat it as a leaf item lookup: resolve the parent module,
find the named item, and render it with full detail.

## Motivation

`cargo brief api clap::builder::Arg` currently fails because `Arg` is a struct,
not a module. Users (and AI agents) naturally write paths to specific types.
This should just work.

## Design

### Resolution algorithm

1. Try resolving the full path as a module (current behavior, unchanged).
2. If module resolution fails, split at the last `::` — parent path + leaf name.
3. Resolve the parent as a module.
4. Search for the leaf name among items in that module.
5. **If leaf is a `Use` (re-export)**: follow the chain to the actual definition.
   Render the resolved item regardless of kind (struct, enum, trait, fn, etc.).
6. Render the leaf item with full detail: definition, fields, impls, methods.

### Ambiguity: module vs item

Module wins. If both a module and an item share the same name under the parent,
module resolution succeeds at step 1, preserving backward compatibility.
This is consistent: `Arg` as a module shows the module; `Arg` as a struct shows
the struct.

### Output scope

When targeting a leaf item:
- Show only the matched item + its inherent impls and trait impls.
- Do NOT show sibling items in the parent module.
- Consistent with module targeting: requesting a module shows that module's
  contents; requesting an item shows that item's contents.

### Use chain following

When the leaf is a `pub use` re-export:
- Follow the re-export to the actual definition.
- Render the resolved definition, not the `use` statement.
- Cross-crate re-exports: use `CrossCrateIndex` if available, otherwise
  follow the Id chain in rustdoc JSON.

## Edge cases

- **Leaf not found**: Error message should list available items in the parent
  module (analogous to how missing modules list available modules).
- **Multiple items with same name**: Rust allows a type and a module to share
  a name. Module wins per above. If multiple non-module items share a name
  (unlikely in practice), show all of them.
- **Enum variants**: `Enum::Variant` should work the same way — resolve `Enum`
  in parent module, then find the variant.

### Result (e9a1ce1) - 26-03-22

Implemented all planned features:

- `CrateModel::find_item_in_module()` — finds non-module items in a parent module, follows Use chains (max 10 hops), returns resolved item or original Use item for foreign refs
- `render::render_leaf_item()` — renders single item + all impl blocks (inherent + trait) with same collapsing as module view
- `render::render_leaf_not_found()` — error listing with visibility-filtered available items
- Pipeline wiring in `run_shared_api_pipeline()`: tries leaf after local module + cross-crate resolution, before "module not found" error. Parent module existence checked to show leaf-not-found vs module-not-found
- 9 new integration tests (165 total): struct/trait/reexport leaf, root-level item, not-found, private visibility, module-wins, two pipeline e2e tests

**Deviations from plan:**
- `render_leaf_not_found` received `same_crate` + `reachable` params (not in original plan) — code review caught private item leakage
- `find_module_entry` made public (was private) — needed by render module for parent module existence checks
- Impl rendering inlined in `render_leaf_item` rather than reusing `render_inlined_impl_blocks` — avoids hardcoded `same_crate=false` coupling, consistent with plan's simplification note
