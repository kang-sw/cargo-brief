# Idea: Reduce boilerplate trait impl noise

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

Group impls that differ only in the type parameter:
```rust
// Before: 8 lines
impl PartialEq<[u8]> for Bytes { .. }
impl PartialEq<str> for Bytes { .. }
impl PartialEq<Vec<u8>> for Bytes { .. }
impl PartialEq<String> for Bytes { .. }
// ... 4 more

// After: 1 line
impl PartialEq<[u8] | str | Vec<u8> | String | ...> for Bytes { .. }
```

### B. Suppress reverse impls

If `impl PartialEq<str> for Bytes` is shown, don't also show
`impl PartialEq<Bytes> for str` — it's implied by symmetry.

### C. Collapse marker trait impls into struct line

```rust
// Before:
pub struct Bytes { .. }
impl Send for Bytes { .. }
impl Sync for Bytes { .. }
impl Clone for Bytes { .. }
impl Debug for Bytes { .. }

// After:
pub struct Bytes { .. }  // Send, Sync, Clone, Debug
```

### D. Default-hide common derive impls

Hide `Debug`, `Clone`, `Copy`, `Default`, `Hash`, `Eq`, `PartialEq`,
`Send`, `Sync`, `Unpin`, `UnwindSafe`, `RefUnwindSafe` unless `--all`.
These are "expected" for most types and rarely influence API usage decisions.

## Complexity

Medium-high. Requires classifying trait impls by "interestingness" and
grouping logic. Best done after where-clause rendering is implemented.
