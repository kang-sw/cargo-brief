# Search Module

## Entry Points
- `src/search.rs` — `render_search_inner()` is the core; all public variants (`render_search`, `render_search_filtered`, `render_search_methods_of`) delegate to it.

## Module Contracts
- Pattern parsing (`parse_pattern`) guarantees: exclusion tokens (`-term`) are collected globally across all OR groups into `ParsedPattern.exclusions`, regardless of which comma-separated group the `-term` token appears in. Writing `"Foo,Bar -test"` removes `-test` matches from results for both `Foo` AND `Bar`.
- `token_matches` with `TokenKind::Exact` checks only the final `::` segment via `rsplit("::").next()`. Writing `=foo::Bar` never matches anything — `=` only binds a bare name, not a path.
- `glob_match` applies the pattern to the full item path string including `::` separators. `*` spans module boundaries: `*Builder*` matches `outer::SomeBuilder::new` because `*` crosses `::`.
- Smart-case is determined once from the entire raw pattern string before parsing: if any character is uppercase, the whole parse is case-sensitive (stored tokens are not lowercased).

## Common Mistakes
- Writing `-term` inside one OR branch expecting it to be scoped to that branch: exclusions are global. `"Spawn,Despawn -test"` removes `-test` from all results, not just `Despawn`.
- Writing `=foo::Bar` to scope an exact match to a module path: only the final segment after the last `::` is checked. `=Bar` is the correct form; use AND (`bar =Bar`) to also require the module path.
- Writing a glob like `outer::*Builder` expecting it to match only items in the `outer` module: glob is applied to the full path string, so `outer::*Builder` does match (literal prefix `outer::` then glob `*Builder`), but `*Builder` alone would also match `inner::SomeBuilder`. Anchor with a literal path prefix when module scoping is needed.
