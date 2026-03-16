---
title: "Search kind filter (--search-kind)"
status: idea
---

# Idea: Search kind filter (`--search-kind`)

## Priority: P2

## Motivation

`--search Request` on http returns 68 results mixing methods, structs, fields,
constants, associated types, and use-lines. No way to say "just show methods"
or "just show types."

`--search Service` on tower returns 16 results including `use` lines that aren't
actionable.

## Design

```
--search-kind <KIND>   Filter search results by item kind
```

Comma-separated, accepts: `fn`, `struct`, `enum`, `trait`, `union`, `field`,
`variant`, `const`, `static`, `type`, `macro`, `use`.

Examples:
- `--search spawn --search-kind fn` — only functions/methods
- `--search Error --search-kind struct,enum` — only type definitions
- `--search TcpStream --search-kind fn` — only TcpStream methods (like --methods-of but works)

## Complexity

Low. Filter on `LeafKind` in `search.rs` before rendering.
