---
title: "Fix --methods-of stack overflow"
status: done
completed: 2026-03-22
---

## Problem

`--methods-of` flag causes stack overflow (exit 134) on any type — both
stdlib types (`Result`) and user-defined types (`ApiArgs`).

```
$ cargo brief search self --methods-of Result
thread 'main' has overflowed its stack
$ cargo brief search self --methods-of ApiArgs
thread 'main' has overflowed its stack
```

## Root Cause

`run_search_pipeline()` in `src/lib.rs:435` called itself recursively when
`methods_of` was set. The `--methods-of` preprocessing intentionally keeps
`methods_of` in the args (needed by `run_shared_search_pipeline` for exact
parent-type matching), so the recursive call re-entered the same branch
infinitely.

Existing tests called `render_search_methods_of` directly, bypassing
`run_search_pipeline` entirely — the bug was never caught.

## Found By

Usability test (E05, E06) — exploratory testing.

## Acceptance Criteria

- `cargo brief search self --methods-of ApiArgs` returns methods without crash
- `cargo brief search self --methods-of Result` either returns results or
  a clean "type not found" message (no panic/stack overflow)
- No regression in existing `cargo test`

### Result (3d79479) - 26-03-22

Replaced recursive `run_search_pipeline(&args, remote)` with inline context
building (`build_local_context_search` / `build_remote_context_search`) +
direct `run_shared_search_pipeline` call. This mirrors the pattern used by
the non-methods-of path 3 lines below.

Added regression test `methods_of_no_stack_overflow` that calls
`run_search_pipeline` (the public entry point) with `methods_of: Some("PubStruct")`
to exercise the exact code path. All 173 tests pass.
