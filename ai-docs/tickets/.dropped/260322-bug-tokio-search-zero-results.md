---
title: "Cross-crate search returns 0 results for tokio::spawn"
status: dropped
---

## Problem

`cargo brief -C search tokio@1 spawn` returns 0 results.

## Resolution: Not a Bug

The 0-results behavior is **correct**. `tokio@1` with default features only
exposes `io`, `net`, and 1 macro. The `task` module (containing `spawn`) requires
the `rt` feature, which is not in the default feature set.

**Working invocation:** `cargo brief -C -F full search tokio@1 spawn` → 14 results.

This is a feature-gating issue, not a search bug. The broader UX concern
(suggesting features when a search returns 0 results for a facade crate) is
tracked in `tickets/.done/260322-feat-facade-crate-default-ux.md`.

## Found By

Usability test (Q03) — quality evaluation of remote crate search.
