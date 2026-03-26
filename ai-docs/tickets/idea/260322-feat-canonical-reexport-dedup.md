---
title: "Canonical path dedup for named re-export expansion"
---

## Problem

Current named re-export expansion uses leaf name (`seen_names: HashSet<String>`)
for deduplication. When different source crates export items with the same leaf
name (e.g., `pub use crate_a::Foo as AFoo; pub use crate_b::Foo as BFoo;`),
only the first is expanded — the second stays as an unexpanded `pub use` line.

Not a data loss issue (the `pub use` line remains navigable), but asymmetric.

## Discussed Approach

Track expansions by canonical item identity (rustdoc `Id` or definition-site path)
instead of leaf name string:

```
already_handled: HashMap<CanonicalPath, AccessPath>
```

- First encounter → expand inline, record where it was expanded
- Subsequent encounters → replace `pub use` with `pub use <recorded_access_path>;`

## Why Deferred

1. **Practical impact is low** — facade crates rarely re-export different items
   with the same leaf name from different source crates
2. **Pre-existing alias issue** — cross-crate aliased re-exports (`as`) already
   lose the alias at render level (render.rs uses `use_item.source`, not alias name)
3. **Implementation cost** — requires canonical path tracking across crate
   boundaries, which the current string-based post-processing can't support
4. **Graceful degradation** — unexpanded `pub use` lines are still navigable
   via `cargo brief api <crate> <Item>`

## Trigger

Revisit when a real-world crate is found where leaf-name dedup causes
user-visible confusion.
