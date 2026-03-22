---
title: "Improve default output for facade crates (serde, clap)"
status: todo
---

## Problem

Facade crates that re-export from sub-crates show only `pub use` lines
at default depth, hiding the actual API surface.

```
$ cargo brief -C api serde
// crate serde[1.0.228]
//! ... (90 lines of crate docs) ...
pub use serde_core::de;
pub use serde_core::ser;
pub use serde_core::Deserialize;
pub use serde_core::Serialize;
pub use serde_core::Serializer;
```

The user sees re-export lines but not the trait definitions, methods, or
module contents. `--expand-glob` or `--depth 2` fixes this, but the default
experience is poor for the primary use case (understanding a crate's API).

Similarly, `clap` shows `pub use clap_builder::Command` without expanding
the type's definition or methods.

## Considerations

- This is a UX quality issue, not a crash/correctness bug
- Automatically expanding all re-exports could produce very large output
- Possible approaches:
  - Auto-detect facade crates (few own items, many re-exports) and suggest
    `--expand-glob` in output
  - Default to `--expand-glob` when root module has only re-exports
  - Show a hint like `// use --expand-glob to see full definitions`
- Re-export paths show `serde_core::` not `serde::` — path resolution
  could also be improved here

## Found By

Usability test (Q02, Q04) — quality evaluation of serde and clap API output.

## Acceptance Criteria

- User gets actionable output from `cargo brief -C api serde` without
  needing to know about `--expand-glob`
- Either auto-expand or clearly suggest the flag
