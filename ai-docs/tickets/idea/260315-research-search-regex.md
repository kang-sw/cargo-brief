---
title: "Search regex/glob pattern support"
status: idea
---

# Search Regex/Glob Pattern Support

## Goal

Allow `--search` to accept regex or glob patterns (e.g., `*Builder*::new`,
`^from_`) for more precise item discovery.

## Status

Deferred — current substring AND matching is intuitive for LLM agents and
covers most use cases. Revisit if users report needing more precise filtering.

## Notes

- Could use `regex` crate with a `--search-regex` flag to avoid breaking
  existing substring behavior
- Glob patterns (`*`, `?`) might be more LLM-friendly than full regex
- Consider cost: adding `regex` as a dependency vs. simple glob matching
