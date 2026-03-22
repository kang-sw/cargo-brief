---
title: "Fix usability test spec issues found in first run"
status: todo
---

## Issues

### S05: Bad search pattern
`BRIEF search self fn` returns 0 results because `fn` is a Rust keyword,
not an item name. Search matches item names, not syntax.

**Fix:** Change to `BRIEF search self pipeline` or `BRIEF search self run`.

### Remote test network requirements
Remote crate tests (`-C` flag) fail in sandbox environments due to blocked
HTTP to crates.io. The spec and skill need to account for this.

**Fix options:**
- Add a setup note: "pre-warm cache with `cargo brief -C api serde` before
  running in sandbox"
- Or: skill prompt should use `dangerouslyDisableSandbox` for the agent
- Or: spec marks remote tests as SKIP when network unavailable

### Q02/Q04: Facade crate criteria
Quality criteria assume expanded output, but default depth only shows
re-exports. Either adjust criteria to match default behavior or specify
`--expand-glob` / `--depth 2` in the test commands.

**Fix:** Add alternative commands with `--expand-glob` or adjust pass
criteria to accept re-export-only output as WARN (not FAIL).

## Found By

First usability test run (2026-03-22).
