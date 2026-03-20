---
title: "Search pattern matching DSL — conditional include/exclude operators"
status: idea
---

# Search Pattern Matching DSL

## Goal

Extend the search pattern syntax beyond simple substring AND/OR matching
to support conditional operators. This would allow users to express more
precise queries without requiring full regex support.

## Motivation

Current matching is purely additive: AND (space-separated) and OR
(comma-separated) on substring containment. No way to express:
- "find X but exclude results containing Y"
- "require result path to contain X in a specific position"

With `trailing_var_arg` now supporting multiple bare pattern args
(v0.5.2+), the `--` separator naturally enables patterns starting with
operator prefixes like `-` or `+`.

## Concept

```sh
# Exclude results containing "Material" from ShaderRef matches
cargo brief search bevy -- ShaderRef -Material

# Require both ShaderRef AND Asset in result
cargo brief search bevy -- +ShaderRef +Asset

# Possible operators:
#   (bare)  — AND substring match (current behavior)
#   -term   — exclude results containing term
#   +term   — explicit AND (same as bare, but clear with --)
#   ^term   — match only at path component boundary
#   =term   — exact name match (not substring)
```

## Design Notes

- Operators should be opt-in: bare words keep current AND-substring behavior
- `--` is required before operator-prefixed patterns (clap `trailing_var_arg`)
- Comma-separated OR still works within a single arg token
- Consider interaction with `--methods-of` and `--search-kind` (future)
- Parse step: split patterns into include/exclude sets before matching

## Complexity

Medium. Pattern parsing is straightforward; the matching logic in
`search.rs` needs a filter pipeline instead of a single `matches()` call.

## Related

- `260315-research-search-regex.md` — regex support (heavier alternative)
- `260316-feat-search-kind-filter.md` — orthogonal kind-level filtering
- `260321-feat-multi-pattern-args.md` — prerequisite (trailing_var_arg)
