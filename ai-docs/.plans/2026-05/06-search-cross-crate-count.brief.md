# Brief: search-cross-crate-count-hotfix

## Intent

Fix search output headers so the reported result count includes cross-crate
re-exported search results appended after the main-crate search output.

## Approach

- Preserve the existing public search API wrappers that return `String`.
- Add internal count-returning render paths for local and cross-crate search.
- Use those internal totals in `run_shared_search_pipeline` so the first header
  count reports `local_total + cross_total`.
- Keep pagination semantics based on total matches, not rendered line count.

## Constraints

- Do not parse rendered output to infer counts.
- Do not change result row formatting.
- Do not introduce network-dependent tests.
- Keep existing direct `search_cross_crate_index(...) -> String` tests working.

## Out of scope

- Showing `--search-kind` in headers.
- Changing virtual-workspace `self` resolution.
- Redesigning multi-crate search output grouping.

## Details

Repro:

```sh
cargo run -- search test-fixture GlobInnerItem --manifest-path test_fixture/Cargo.toml
```

Current output reports `(0 results)` while printing a cross-crate re-exported
`GlobInnerItem` row. The expected header count is `(1 results)`.

Add regression coverage through `run_search_pipeline`, not only direct
`search_cross_crate_index`, so the full output merge path is exercised.

Acceptance tests:

- A full-pipeline search for `GlobInnerItem` against `test_fixture/Cargo.toml`
  reports `(1 results)` in the first header and still prints `GlobInnerItem`.
- A limited full-pipeline search still reports total matches rather than rendered
  line count.
- Existing search integration tests continue to pass.

## References

- `src/lib.rs` - `run_shared_search_pipeline` appends cross-crate output after
  local search output.
- `src/search.rs` - local search computes a total for the header; cross-crate
  search currently returns only rendered body text.
- `tests/integration.rs` - full-pipeline and cross-crate search regression tests.
- `ai-docs/mental-model/search.md` - local and cross-crate search coupling.
- `ai-docs/mental-model/testing.md` - fixture and integration test conventions.
