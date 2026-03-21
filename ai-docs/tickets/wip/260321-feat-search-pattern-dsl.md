---
title: "Search pattern DSL — glob wildcards, exclusion, exact match"
status: wip
started: 2026-03-21
---

# Search Pattern DSL

## Priority: P2

## Motivation

Current search matching is purely additive substring containment (AND/OR).
No way to express:
- "find spawn but exclude test-related items" (`spawn -test`)
- "match function names starting with from_" (`from_*`)
- "find exactly Router, not RouterService" (`=Router`)

LLM agents need these to narrow results on large crates (e.g., http returns
68 results for "Request").

## Design

Extends existing pattern syntax (space=AND, comma=OR, smart-case) with
three new features:

### 1. Glob wildcards (`*`, `?`)

- `*` matches zero or more characters (within a path, including `::`)
- `?` matches exactly one character
- No character classes (`[a-z]`) — keep it simple
- Applied to the full item path (e.g., `outer::PubStruct::pub_method`)
- When a token contains `*` or `?`, switch from substring to glob matching
  for that token only. Bare words without wildcards remain substring matches.

```sh
cargo brief search bevy "Shader*Ref"
cargo brief search tokio "spawn?"
cargo brief search bevy "*Builder*::new"
```

### 2. Exclusion (`-term`)

- `-term` removes results whose path contains `term`
- Glob wildcards work inside exclusion: `-*Test*`
- Applied after all include matching (AND/OR)
- Requires `--` separator when used as a bare arg (clap trailing_var_arg)

```sh
cargo brief search bevy -- ShaderRef -Material
cargo brief search tokio spawn -test -bench
```

### 3. Exact name match (`=term`)

- `=term` matches only if the final path component (after last `::`) equals
  `term` exactly
- Useful for finding a type without its methods/fields/variants
- Smart-case still applies to the comparison

```sh
cargo brief search bevy -- =Router
cargo brief search http -- =Request --search-kind struct
```

### Operator summary

| Syntax | Meaning | Matching |
|--------|---------|----------|
| `word` | include, substring | path contains "word" |
| `w*ld` | include, glob | path matches glob |
| `-word` | exclude, substring | path must NOT contain "word" |
| `-w*ld` | exclude, glob | path must NOT match glob |
| `=word` | include, exact name | last `::` segment equals "word" |

### Interaction with existing features

- AND/OR grouping: exclusions and exact matches participate in the same
  comma/space parsing. Within an OR group, `-term` excludes from that
  group's matches. `=term` is an AND condition within its group.
- `--methods-of`: orthogonal, applied after pattern matching
- `--search-kind`: orthogonal, applied after pattern matching
- Smart-case: applies to all tokens including glob and exclusion

## Implementation notes

- Glob matching: convert `*` → `.*`, `?` → `.`, wrap in `^...$` for
  full-path match, compile as regex. Or implement simple glob state machine.
  regex crate is already an indirect dependency — direct use is acceptable.
- Pattern parsing: in `render_search_inner`, split tokens into
  (includes, excludes, exact) before building the match predicate.
- No new CLI flags needed — operators are embedded in pattern syntax.

## Supersedes

- `260315-research-search-regex.md` (glob covers the practical use cases)
- `260321-feat-search-pattern-dsl.md` (merged into this ticket)

## Complexity

Medium. Pattern parsing + glob-to-regex conversion + filter pipeline.
~80 lines of new matching logic in `search.rs`.

### Result - 26-03-21

Implemented all three operators: glob (`*`/`?`), exclusion (`-term`), exact (`=term`).

- `TokenKind` enum + `ParsedPattern` struct + `parse_pattern()` + `glob_match()` + `token_matches()` added to `src/search.rs` after `parse_search_limit()`.
- Replaced the old substring-only OR/AND matching block in `render_search_inner()` with `parse_pattern()` dispatch + global exclusion post-filter.
- Glob matching uses iterative two-pointer algorithm (~25 lines), no regex dependency needed.
- Exclusions are global across OR groups as designed.
- Smart-case applies uniformly to all token types.
- Updated CLI `after_help` text with operator docs and examples.
- 15 unit tests for `glob_match` + `parse_pattern`, 12 integration tests covering all operators and combinations.
- All 146 integration tests pass. No clippy regressions.
