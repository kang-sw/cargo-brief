---
title: "UX: Module path targeting silently ignored for remote crates"
status: done
completed: 2026-03-18
---

# UX: Module path targeting silently ignored for remote crates

## Priority: P2

## Problem

When a user specifies a module path with `--crates`, the module path is silently
ignored if it doesn't exist in the crate's module index (e.g., facade crates that
re-export from private modules).

Example: `cargo brief --crates axum@0.8 axum::routing --compact`
- Expected: show only the `routing` module, or an error if not found
- Actual: silently shows the entire crate root (no error, no warning)

This confuses agents and users who expect targeted output or an error message.

## Root Cause

`render_module_api()` falls back to root when `find_module()` returns `None` for
unresolvable paths. For remote crates, the module path argument goes through
`args.module_path` but facade crates hide internal modules behind re-exports.

## Possible Fixes

1. **Show a warning** when the specified module path isn't found, listing available
   modules (already implemented for local crates — check if it triggers for remote)
2. **Suggest `--search`** in the error message as an alternative for facade crates
3. **Fuzzy match** module names (e.g., `routing` → `routing::method_routing`)

## Discovered By

Naive-agent testing (2026-03-17): agent tried `cargo brief --crates axum@0.8
axum::routing` and was confused when it got the full crate instead.

### Result - 26-03-18

Added a `// TIP: Try --search "<path>" ...` line after the available modules listing
in `render_module_api()`. The module-not-found error already worked for both local and
remote crates; the missing piece was suggesting `--search` as an alternative for facade
crates where the module list is unhelpful.
