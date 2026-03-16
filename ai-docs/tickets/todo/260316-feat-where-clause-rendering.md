---
title: "Render where clauses / generic bounds"
status: todo
---

# Feature: Render where clauses / generic bounds

## Priority: P1

## Motivation

Generic function signatures currently omit trait bounds, making them incomplete
for code generation. This is the **#1 missing piece** for LLM consumers.

**Example (tokio --search spawn):**
```
fn task::spawn::spawn<F>(future: F) -> JoinHandle<<F as >::Output>;
```
Missing: `F: Future + Send + 'static, F::Output: Send + 'static`

**Example (bytes):**
```
fn Bytes::slice<impl RangeBounds<usize>: RangeBounds<usize>>(...) -> Self;
```
The bound is technically there but mangled.

An LLM trying to call `tokio::spawn()` cannot know the `Send + 'static`
requirement without bounds, leading to compile errors.

## Data Source

`rustdoc_types::Generics` already contains:
- `params: Vec<GenericParamDef>` — inline bounds (`T: Clone`)
- `where_predicates: Vec<WherePredicate>` — where clause entries

The current `format_generics()` in `render.rs` renders `params` (with bounds)
but **ignores `where_predicates` entirely**.

## Design

### Normal render mode

Append `where` clause after the function signature, before the `;`:

```rust
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static;
```

For single-predicate cases, keep it on one line:
```rust
pub fn from_iter<I>(iter: I) -> Self where I: IntoIterator<Item = u8>;
```

### Search mode

One-liner format can't fit full where clauses. Options:
1. **Compact where**: `fn spawn<F: Future + Send + 'static>(future: F) -> ...`
   (inline bounds into generic params where possible)
2. **Truncated**: `fn spawn<F>(future: F) -> ... where F: Future + Send + ..`
3. **Omit**: keep current behavior (no bounds in search one-liners)

Recommend option 1 for search mode — merge where predicates back into
generic params when they match a param name.

### Implementation

1. Extend `format_generics()` to also return where predicates
2. Add `format_where_clause()` function
3. Wire into `format_function_sig()`, `render_struct()`, `render_trait()`, etc.
4. For search mode, merge simple predicates into generic param bounds

## Files to Modify

| File | Changes |
|------|---------|
| `src/render.rs` → `format_generics()` | Extract where predicates |
| `src/render.rs` → new `format_where_clause()` | Render where clause |
| `src/render.rs` → `format_function_sig()` | Append where clause |
| `src/render.rs` → struct/trait/impl renderers | Append where clauses |
| `src/search.rs` → `render_function_leaf()` | Compact bounds in search |
| `tests/integration.rs` | Test where clause rendering |
