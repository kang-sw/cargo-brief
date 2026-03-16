# Idea: Render crate-level `//!` documentation

## Priority: P2

## Motivation

Crate-level doc comments (`//!` in `lib.rs`) are currently never rendered.
For proc-macro crates this is especially damaging:

- **thiserror**: output is `pub use thiserror_impl::*;` (2 lines). The crate's
  entire usage documentation (`#[derive(Error)]`, `#[error("...")]`, `#[from]`,
  `#[source]`, `#[backtrace]`) lives in the `//!` docs. Currently invisible.
- **serde**: derive attribute documentation (`#[serde(rename = "...")]`) is in
  crate-level docs.
- **tokio**: getting-started examples, feature flag documentation.

## Design

Render `Crate.root_doc` (or the root module's `docs` field) as a header block
after the `// crate <name>` line:

```
// crate thiserror
//! Derive macro for the standard library's `std::error::Error` trait.
//!
//! # Example
//! ...
```

Respects `--no-docs`, `--compact`, `--doc-lines N`.

## Data Source

`rustdoc_types::Crate` → root module `Item` → `docs: Option<String>`

## Complexity

Low. Read root module docs, render with `//!` prefix.
