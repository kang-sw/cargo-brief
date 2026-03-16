# Fix: `--methods-of` stack overflow (infinite recursion)

## Priority: P0 (crash bug)

## Problem

`--methods-of <TYPE>` crashes with stack overflow on **every** invocation.
Confirmed on: tokio TcpStream, axum Router, http Request, bytes Bytes/BufMut.

## Root Cause

`src/lib.rs:28-42` — `run_pipeline()` translates `--methods-of` into `--search`
+ exclusion flags, then recursively calls `run_pipeline(&args)`. But
`args.methods_of` is never set to `None`, so the recursive call hits the same
`if let Some(type_name) = &args.methods_of` branch infinitely.

```rust
// line 30-41
if let Some(type_name) = &args.methods_of {
    let mut args = args.clone();
    args.search = Some(type_name.clone());
    args.no_structs = true;
    // ... exclusion flags ...
    // BUG: args.methods_of is still Some(...)
    return run_pipeline(&args);  // infinite recursion
}
```

## Fix

One line: add `args.methods_of = None;` before line 41.

## Verification

1. `cargo test` — all pass
2. Manual: `cargo brief --crates bytes@1 --methods-of Bytes` — no crash, shows methods
3. Manual: `cargo brief --crates http@1 --methods-of Request` — no crash
