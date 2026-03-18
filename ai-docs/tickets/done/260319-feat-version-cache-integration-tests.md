---
title: "Integration tests for version-normalized cache + crates.io resolution"
status: done
started: 2026-03-19
completed: 2026-03-19
---

# Integration tests for version-normalized cache + crates.io resolution

## Priority: P1

## Motivation

The v0.4.1 version-normalized cache feature (crates.io API resolution, normalized dir naming,
glob-based `--clean`, cache reuse across spec forms) was verified only manually. These behaviors
should have automated test coverage to prevent regressions.

## Scope

Add tests at two layers:

1. **Unit tests (no network):** `clean_cache` glob matching with pre-seeded fake directories.
2. **Integration tests (`#[ignore]`, network required):** `fetch_resolved_version`, `resolve_workspace`
   dir naming/reuse, version cache file creation, full pipeline header verification.

All network-dependent tests use `#[ignore]` and run via `cargo test -- --ignored`.
`CARGO_BRIEF_CACHE_DIR` pointed at a tempdir for isolation.

### Result (9eceec5) - 26-03-19

Implemented all planned tests:

**Unit tests (`src/remote.rs`):**
- `test_clean_cache_glob_matching` — seeds fake `serde[*]` and `tokio[*]` dirs, verifies selective removal by name prefix + version cache cleanup
- `test_clean_cache_empty_spec_removes_all` — verifies empty spec removes entire cache directory

**Integration tests (`tests/version_cache_integration.rs`, 8 tests, all `#[ignore]`):**
1. `fetch_resolved_version_bare_spec` — wildcard resolves to valid semver
2. `fetch_resolved_version_major_range` — major pin returns matching version
3. `resolve_workspace_creates_normalized_dir` — exact pin creates `name[version]` dir
4. `resolve_workspace_same_version_reuses_dir` — bare and `@1` specs resolve to same dir
5. `resolve_workspace_features_in_dir_name` — features alpha-sorted in dir name
6. `version_cache_file_created` — `versions/serde.json` created with valid JSON
7. `clean_cache_removes_matching_dirs` — network-seeded dirs cleaned by name
8. `full_pipeline_header_shows_version` — full pipeline output starts with versioned header

**Note:** Env-var-based cache isolation requires `--test-threads=1` for unit tests to avoid race conditions. Integration tests are `#[ignore]` so they run individually.
