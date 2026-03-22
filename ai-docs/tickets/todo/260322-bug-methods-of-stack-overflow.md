---
title: "Fix --methods-of stack overflow"
status: todo
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

## Likely Cause

Recursive type resolution loop in the search path. `--methods-of` probably
follows type references without cycle detection, causing infinite recursion
when encountering self-referential or deeply nested types.

## Found By

Usability test (E05, E06) — exploratory testing.

## Acceptance Criteria

- `cargo brief search self --methods-of ApiArgs` returns methods without crash
- `cargo brief search self --methods-of Result` either returns results or
  a clean "type not found" message (no panic/stack overflow)
- No regression in existing `cargo test`
