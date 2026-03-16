# Fix: Rendering artifacts (empty trait path, $crate leak, impl Trait desugar)

## Priority: P1

## Problems

Three distinct rendering bugs that produce confusing pseudo-Rust output:

### 1. Empty trait path in associated type projections

`<F as >::Output` instead of `<F as Future>::Output` or `F::Output`.

**Where it appears:**
- tokio `spawn<F>` → `JoinHandle<<F as >::Output>`
- axum `Service` impls → `<Self as >::Future`
- tower `ServiceBuilder::service()` → `<L as >::Service`

**Source:** `format_type()` in `src/render.rs` — the `QualifiedPath` rendering
likely receives an empty or unresolved trait path from rustdoc JSON. Need to
check how `QualifiedPath { self_type, trait_, name, args }` is handled when
`trait_` is missing or empty.

### 2. `$crate::` prefix leaking from macro expansion

`impl<L: $crate::clone::Clone> Clone for ServiceBuilder<L>` — macro hygiene
artifacts should be normalized to `std::`/`core::` or stripped entirely.

**Where it appears:** Any crate using `#[derive(Clone)]`, `#[derive(Debug)]` etc.
on generic types. Observed in tower, axum.

**Source:** `format_path()` in `src/render.rs` — the path segments from
rustdoc JSON include raw `$crate` prefixes. Need path normalization.

### 3. `impl Trait` desugared into redundant generic

`pub fn slice<impl RangeBounds<usize>: RangeBounds<usize>>(...)` instead of
`pub fn slice(range: impl RangeBounds<usize>) -> Self`.

**Where it appears:** bytes `Bytes::slice()`, other crates with `impl Trait` args.

**Source:** `format_function_sig()` — rustdoc JSON may desugar `impl Trait`
arguments into synthetic generic params. Need to detect and re-sugar these.

## Files to Investigate

| File | Area |
|------|------|
| `src/render.rs` → `format_type()` | QualifiedPath handling |
| `src/render.rs` → `format_path()` | `$crate` normalization |
| `src/render.rs` → `format_function_sig()` / `format_generics()` | impl Trait re-sugaring |

## Verification

1. `cargo brief --crates tower@0.5 --no-docs` — no `$crate::`, no `<L as >::Service`
2. `cargo brief --crates bytes@1 --search slice` — shows `impl RangeBounds<usize>`
3. `cargo brief --crates tokio@1 --features rt --search spawn` — shows `F::Output`
