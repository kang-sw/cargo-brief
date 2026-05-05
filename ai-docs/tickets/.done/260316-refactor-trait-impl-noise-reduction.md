---
title: "Reduce boilerplate trait impl noise"
status: done
completed: 2026-03-18
---

# Reduce boilerplate trait impl noise

## Priority: P2

## Motivation

Large crates devote significant output to low-value trait impls:

**bytes `Bytes`:** 20+ `PartialEq<X>` impls × 2 directions = ~40 lines that
convey one fact: "Bytes is comparable with byte-like types."

**axum extractors:** Every type has `Debug`, `Clone`, `Copy`, `Default` impls
with `$crate::` prefixed bounds — pure boilerplate.

**anyhow `Error`:** `UnwindSafe`, `RefUnwindSafe`, `Drop` — rarely actionable.

## Ideas

### A. Collapse repetitive impls
### B. Suppress reverse impls
### C. Collapse marker trait impls into struct line
### D. Default-hide common derive impls

## Complexity

Medium-high. Requires classifying trait impls by "interestingness" and
grouping logic. Best done after where-clause rendering is implemented.

### Result (pending) - 26-03-18

Implemented approach combining A+C: all simple trait impls (no associated
types/constants) are collapsed into per-type summary comments, grouped by
`for_` type. Forward impls show just trait name, reverse impls show
`Trait for ForType`. Rich trait impls (with assoc items) remain expanded.
`--all` disables collapsing. Negative impls never collapsed.

Results on `bytes --recursive`: 120 impl lines → 8 expanded + ~18 summary lines.
4 new tests + 2 updated, 105 total passing.
