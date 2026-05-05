---
title: "Expand named cross-crate re-exports inline (like glob expansion)"
status: done
completed: 2026-03-22
---

## Problem

Glob re-exports (`pub use serde_core::*;`) are expanded inline by default,
but named re-exports (`pub use serde_core::Serialize;`) are not. This creates
inconsistent output for facade crates that mix both styles.

serde uses named re-exports for its primary types, so `cargo brief -C api serde`
shows `pub use serde_core::Serialize;` instead of the full trait definition.
Users must know to search or navigate to the source crate manually.

## Example

```
$ cargo brief -C api serde
// crate serde
pub use serde_core::Serialize;    // ← not expanded
pub use serde_core::Deserialize;  // ← not expanded
pub use serde_core::de;
pub use serde_core::ser;
```

Expected (with expansion):
```
$ cargo brief -C api serde
// crate serde
pub trait Serialize { ... }       // ← inlined from serde_core
pub trait Deserialize<'de> { ... }
pub use serde_core::de;
pub use serde_core::ser;
```

## Approach

Extend the existing glob expansion pipeline to also handle named cross-crate
re-exports. The source crate JSON generation and rendering infrastructure
already exists — named re-exports just need to be collected alongside globs
and matched by their specific `pub use source::ItemName;` pattern.

Key changes:
- `expand_glob_reexports()`: also collect named cross-crate re-exports
- `apply_glob_expansions()`: match `pub use {source}::{name};` lines and
  replace with inlined definitions
- `GlobExpansionResult`: track named re-export targets

## Found By

Usability test Q02 — serde API output shows re-export lines instead of
trait definitions.

## Acceptance Criteria

- Named cross-crate re-exports are expanded inline by default
- `--no-expand-glob` suppresses both glob and named expansion
- serde shows `Serialize`/`Deserialize` trait definitions without extra flags

### Result (332002e) - 26-03-22

Implemented named cross-crate re-export expansion as an extension of the
existing glob expansion pipeline.

**What was implemented:**
- `GlobExpansionResult.named_reexports` field for `(item_name, full_source_path)` tuples
- Second pass in `expand_glob_reexports()` detecting non-glob cross-crate Use items
- `render::render_single_inlined_item()` for rendering individual named items from source models
- Named expansion in `apply_glob_expansions()` replacing `pub use {source};` lines
- Test fixture: `named-source` sub-crate with NamedSourceItem/NamedSourceTrait
- 3 integration tests + 1 serde facade test
- serde dependency added to test_workspace for facade testing

**Key finding during implementation:**
Phase 2 glob loop must iterate `item_names.keys()` (not `source_models`) to avoid
poisoning `seen_names` with named-only source items. The shared `source_models` map
contains entries from both passes, but only glob entries have a `pub use {source}::*;`
pattern to replace.

**Known limitations:**
- Only root-level items in the source crate are expandable. Nested paths like
  `pub use foo::bar::Baz;` won't expand (item stays as `pub use` line).
- Aliased re-exports (`pub use foo::Bar as Baz;`) lose the alias when expanded.
- Module re-exports preserved as-is (handled by `render_single_inlined_item` returning None).
