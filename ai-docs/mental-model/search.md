---
domain: Search Module
description: Pattern parsing, leaf item walker, one-line-per-item renderer, cross-crate index search
sources:
  - src/search.rs
  - src/lib.rs
related:
  - visibility.md
---

# Search Module

## Entry Points
- `src/search.rs` — `render_search_inner()` is the core; all public variants (`render_search`, `render_search_filtered`, `render_search_methods_of`) delegate to it. `search_cross_crate_index()` mirrors the same logic for the cross-crate path.

## Module Contracts
- Pattern parsing (`parse_pattern`) guarantees: exclusion tokens (`-term`) are collected globally across all OR groups into `ParsedPattern.exclusions`, regardless of which comma-separated group the `-term` token appears in. Writing `"Foo,Bar -test"` removes `-test` matches from results for both `Foo` AND `Bar`.
- `--in-params` and `--in-returns` reuse `parse_pattern`, but the subject is a rendered type string from `render::format_type_pub`, not the item path. Exclusions are scoped to the owning filter subject: name-pattern exclusions inspect item paths, parameter exclusions inspect one candidate parameter type at a time, and return exclusions inspect the return type.
- For `--in-params`, `matches_type_filter()` accepts a function when one parameter type satisfies the whole parsed pattern, including exclusions. It does not apply exclusions as a function-wide scan across all parameters.
- `token_matches` with `TokenKind::Exact` checks only the final `::` segment via `rsplit("::").next()`. Writing `=foo::Bar` never matches anything — `=` only binds a bare name, not a path.
- `glob_match` applies the pattern to the full item path string including `::` separators. `*` spans module boundaries: `*Builder*` matches `outer::SomeBuilder::new` because `*` crosses `::`.
- Smart-case is determined once from the entire raw pattern string before parsing: if any character is uppercase, the whole parse is case-sensitive (stored tokens are not lowercased).
- `is_member()` defines the member/non-member boundary: `Field`, `Variant`, `AssocType`, `AssocConst` are always members; `Function` is a member only when `context == ImplMethod`. Free functions (context `None`) are not members.
- Default member suppression contract (no `--members`, no `--methods-of`): member items in `matched` are silently dropped unless a search token (Substring or Exact) matches exactly the member's final `::` segment name. Glob tokens never trigger member retention. Parent types are then injected as context headers for any surviving orphan members.
- `--members` contract: matched types (Struct/Enum/Trait/Union) are expanded with all children from `leaves`; sort switches to path-based order so members appear directly after their parent.
- `--methods-of` disables default member suppression entirely — all matched members pass through.
- Cross-crate member walking: `search_cross_crate_index` populates `type_leaves` with struct fields (`walk_struct_fields_pub` — public-only), enum variants, and union fields before the filtering phase, mirroring local-crate behavior. Enum variant detail fields are resolved from the index at render time, not at walk time.
- Collapsed display: when consecutive items in the sorted output share the same parent path and the current item `is_member()`, a `-::member_name` continuation line is emitted instead of a full `render_leaf` line. The padding width is `parent_path.len() - 1`. This is purely a display effect applied after all filtering.

## Coupling
- `render_search_filtered` and `search_cross_crate_index` both take `members: bool` as their final argument. Adding any new filtering option to one function must be mirrored in both, or cross-crate results will diverge from local results silently.
- Member filtering runs after exclusion and before `--methods-of` filtering. Changing this order alters which members are visible when both flags are combined.
- `render_collapsed_member` calls `render::format_type_pub` and `render::format_function_sig_pub` directly. If the render module changes those function signatures, collapsed display silently produces wrong output without a compile error (the return type is `String` in both cases).
- Type-filter matching also depends on `render::format_type_pub`; changing rendered type strings can change `--in-params` / `--in-returns` matches even if search traversal is untouched.

## Extension Points & Change Recipes
- **Add a new member kind**: touch `is_member()`, `render_collapsed_member` (add match arm), and both member-expansion blocks in `render_search_inner` and `search_cross_crate_index`. Missing `render_collapsed_member` arm → new members in `--members` mode emit nothing (silent blank line).
- **Add a new type kind eligible for `--members` expansion**: add the kind to the `type_paths` collection filter in both `render_search_inner` and `search_cross_crate_index`, and add a walk function for its children. Missing the cross-crate walk → `--members` expands the type in local results but not in cross-crate results.

## Common Mistakes
- Writing `-term` inside one OR branch expecting it to be scoped to that branch: exclusions are global. `"Spawn,Despawn -test"` removes `-test` from all results, not just `Despawn`.
- Writing `=foo::Bar` to scope an exact match to a module path: only the final segment after the last `::` is checked. `=Bar` is the correct form; use AND (`bar =Bar`) to also require the module path.
- Writing a glob like `outer::*Builder` expecting it to match only items in the `outer` module: glob is applied to the full path string, so `outer::*Builder` does match (literal prefix `outer::` then glob `*Builder`), but `*Builder` alone would also match `inner::SomeBuilder`. Anchor with a literal path prefix when module scoping is needed.
- Searching for a field name (e.g. `my_field`) without `--members`: the field will be suppressed by default member filtering unless `my_field` exactly matches a token in the search pattern. Use `--members` or `--methods-of` to surface members when searching for type names.
- Expecting `--members` to surface private fields in cross-crate output: `walk_struct_fields_pub` skips non-public fields. Private fields of external crates are never included regardless of `--at-mod`.
