---
title: "search: complete signature type filters from PR #5"
spec:
  - 260423-search-subcommand
  - 260423-search-pattern-dsl
  - 260423-search-output-format
related-mental-model:
  - search
---

# search: complete signature type filters from PR #5

## Background

PR #5 adds `search --in-params <PATTERN>` and `search --in-returns <PATTERN>`.
The direction is right: agents often need to find functions by signature shape,
not only by item path. The PR also correctly threads the filters through both
local and cross-crate search paths, which matches the search module coupling
rule.

The integration branch already includes the upstream PR. This ticket tracks the
remaining work needed to make the new caller-visible behavior complete and
documented.

## Decisions

- Keep the filters on `search`, not `code`; this is API-surface search over
  rustdoc types.
- Reuse the existing search pattern DSL where possible so users do not learn a
  separate type-pattern language.
- Preserve AND semantics across name pattern, parameter filter, and return
  filter.
- Keep local and cross-crate behavior in lockstep.

## Phases

### Phase 1: Specify the new CLI behavior

Update the CLI and output/search specs for `--in-params` and `--in-returns`.
The spec must settle whether type filters support the full existing pattern DSL,
including exclusions, or only the positive OR/AND token subset currently wired
by the PR.

Success criteria:

- `ai-docs/spec/cli-surface.md` documents both flags and empty-name-pattern
  behavior.
- `ai-docs/spec/output-format.md` documents any output/header implications if
  a type-filter-only search displays an empty search pattern.
- The ticket `spec:` frontmatter remains aligned with the final spec stems.

### Phase 2: Harden type-filter semantics and tests

Bring implementation and tests into line with the settled spec. Cover local and
cross-crate parity, empty name patterns, combined param/return filters, member
method behavior, and any exclusion semantics chosen in Phase 1.

Success criteria:

- Type filters behave consistently in local and cross-crate search.
- Tests cover free functions and member methods, not only non-member functions.
- Remote or facade-style search coverage proves the new args are wired through
  `run_search_pipeline`, not only direct `search_cross_crate_index` calls.
- `cargo test` passes.

### Phase 3: Update user-facing release docs

Update README and CHANGELOG entries for the new search flags after the behavior
is finalized.

Success criteria:

- README includes concise examples for finding functions by parameter and
  return type.
- CHANGELOG records the new caller-visible search capability.

