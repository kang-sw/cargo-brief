---
title: "Improve default output for facade crates (serde, clap)"
status: done
started: 2026-03-22
completed: 2026-03-22
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
- Re-export paths show `serde_core::` not `serde::` — path resolution
  could also be improved here (tracked by cross-crate accessible paths feature)

## Found By

Usability test (Q02, Q04) — quality evaluation of serde and clap API output.

## Acceptance Criteria

- User gets actionable output from `cargo brief -C api serde` without
  needing to know about `--expand-glob`
- Either auto-expand or clearly suggest the flag

### Result (a7c84bc) - 26-03-22

Flipped the CLI flag from `--expand-glob` (opt-in) to `--no-expand-glob` (opt-out).
Glob re-export expansion now defaults to on for both local and remote modes.

- `src/cli.rs`: `expand_glob: bool` → `no_expand_glob: bool`
- `src/lib.rs`: Two call sites pass `!args.no_expand_glob` to `apply_glob_expansions()`
- Internal `apply_glob_expansions()` function signature unchanged
- All 7 test files updated; facade tests now exercise default expansion behavior
- Phase 1 test tightened with assertion verifying pub use lines in opt-out mode

**Deviation from plan:** The plan's Phase 1 test assertion
`!output.contains("struct GlobSourceItem")` was incorrect — the
`ReachableInfo.glob_inlined` path inlines definitions at render level
independently of `--no-expand-glob`. Changed to verify presence of `pub use`
lines instead.

**Remaining concern:** Re-export path resolution (`serde_core::` vs `serde::`)
is a separate issue handled by the cross-crate accessible paths feature.
