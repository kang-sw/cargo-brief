---
title: "search: complete signature type filters from PR #5"
spec:
  - 260423-search-subcommand
  - 260423-search-pattern-dsl
  - 260423-search-output-format
related-mental-model:
  - search
completed: 2026-05-06
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

The upstream PR did not perform the repository documentation step: it adds
caller-visible CLI behavior without spec, README, or changelog updates. Treat
that omission as part of the local integration scope, not as a decision to skip
documentation.

## Decisions

- Keep the filters on `search`, not `code`; this is API-surface search over
  rustdoc types.
- Reuse the existing search pattern DSL where possible so users do not learn a
  separate type-pattern language.
- Type filters support the same positive tokens and exclusion tokens as the
  existing search pattern DSL. The match subject changes from item path to the
  rendered type string for the specific filter.
- Preserve AND semantics across name pattern, parameter filter, and return
  filter.
- Scope exclusions to the filter that owns them: name-pattern exclusions apply
  to item paths, `--in-params` exclusions apply to each candidate parameter type
  string, and `--in-returns` exclusions apply to the return type string.
- For `--in-params`, one parameter type must satisfy the whole type-filter
  pattern. Do not treat exclusions as function-wide checks across all
  parameters.
- Because `--in-params` and `--in-returns` each accept one CLI option value,
  multi-token type patterns require shell quoting, for example
  `--in-params "TokenStream -Option"`. Unquoted `-Option` is a separate CLI
  token, not part of the type-filter pattern.
- Search output headers should include active type filters so type-filter-only
  searches do not render as unexplained `search: ""` results.
- Keep local and cross-crate behavior in lockstep.

## Phases

### Phase 1: Specify the new CLI behavior

Update the CLI and output/search specs for `--in-params` and `--in-returns`.
Type filters must reuse the existing pattern DSL, including exclusions, with the
match subject scoped to the relevant rendered type string. Document that
multi-token type patterns require quotes because each flag accepts one option
value.

Update the search help text to reduce CLI-tokenization ambiguity. The help text
should make clear that exclusion tokens inside type-filter patterns require
quoting, for example `--in-params "TokenStream -Option"`.

Specify the output header extension for active type filters. Name-only searches
keep the existing header. When type filters are active, append only the active
filters before the result count, for example:

```
// crate cargo_brief — search: "" in-params: "PathBuf" (7 results)
// crate cargo_brief — search: "parse" in-params: "TokenStream -Option" in-returns: "Result" (2 results)
```

This phase exists because the external PR did no documentation pass before
landing on the integration branch.

Success criteria:

- `ai-docs/spec/cli-surface.md` documents both flags and empty-name-pattern
  behavior, including quote requirements for multi-token type patterns.
- `ai-docs/spec/cli-surface.md` documents filter-scoped exclusion behavior and
  the parameter-level subject for `--in-params`.
- `ai-docs/spec/output-format.md` documents the active-filter header extension.
- The ticket `spec:` frontmatter remains aligned with the final spec stems.

### Result (b0e126d) - 2026-05-06

Updated the CLI and output specs for `--in-params` and `--in-returns`.
The spec now records empty-name-pattern behavior, filter-scoped exclusions,
parameter-level matching for `--in-params`, quote requirements for multi-token
type-filter patterns, and active-filter search headers.

### Phase 2: Harden type-filter semantics and tests

Bring implementation and tests into line with the settled spec. Cover local and
cross-crate parity, empty name patterns, combined param/return filters, member
method behavior, and any exclusion semantics chosen in Phase 1.

Success criteria:

- Type filters behave consistently in local and cross-crate search.
- Type-filter exclusions apply to rendered type strings, not item paths.
- `--in-params` accepts a function when one parameter type satisfies the full
  parameter pattern, including exclusions.
- Tests cover free functions and member methods, not only non-member functions.
- Tests cover empty name pattern plus type filters, combined name/param/return
  AND semantics, and active-filter output headers.
- Remote or facade-style search coverage proves the new args are wired through
  `run_search_pipeline`, not only direct `search_cross_crate_index` calls.
- `cargo test` passes.

### Result (f5d9437) - 2026-05-06

Implemented filter-scoped type-filter exclusions and active-filter search
headers. Added help text that calls out quoted multi-token type-filter patterns.
Expanded tests for local and cross-crate type-filter exclusions, parameter-level
matching, member methods, empty name patterns, active-filter headers, and help
text. write-code review completed clean after one test-coverage follow-up.

### Phase 3: Update user-facing release docs

Update README and CHANGELOG entries for the new search flags after the behavior
is finalized.

Success criteria:

- README includes concise examples for finding functions by parameter and
  return type, including a quoted exclusion example.
- CHANGELOG records the new caller-visible search capability.

### Result (9ef48e8) - 2026-05-06

Updated README examples and the Unreleased changelog entry for search signature
type filters, including quoted exclusion syntax. Updated the search mental model
for rendered-type matching, filter-scoped exclusions, and `format_type_pub`
coupling.
