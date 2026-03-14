# Remote Pipeline

## Entry Points
- `src/lib.rs:100-142` — `run_remote_pipeline()` (private helper).
- `src/remote.rs` — `parse_crate_spec()`, `create_temp_workspace()`.

## Module Contracts
- `run_remote_pipeline()` guarantees: `TempDir` is held alive for the entire function scope. All borrowed paths (`tmp_manifest_str`) are valid for all downstream calls including glob expansion.
- Remote pipeline always uses `document_private_items=false` and `same_crate=false`. These two flags MUST stay in sync — see `visibility.md`.

## Coupling
- `TempDir` lifetime → all downstream calls: `tmp_manifest` (a `PathBuf`) is derived from `tmp.path()`, and `tmp_manifest_str` (a `Cow<str>`) borrows from `tmp_manifest`. The borrow chain is `tmp` → `tmp_manifest` → `tmp_manifest_str`. All three must live for the entire function. `load_cargo_metadata`, `generate_rustdoc_json`, `expand_glob_reexports`, and `apply_glob_expansions` all receive this borrow. Dropping any link early → "file not found" runtime error.
- `parse_crate_spec()` version semantics: bare name → `"*"`, `name@version` with fewer than 2 dots → verbatim (e.g., `serde@1` → `"1"`), `name@x.y.z` (2+ dots) → `"=x.y.z"` (exact pin).

## Extension Points & Change Recipes
- **Add feature flag support**: Modify `create_temp_workspace()` to include `features = [...]` in the generated Cargo.toml. Add `--features` flag to `BriefArgs`.
- **Add caching**: Replace `TempDir` with a persistent cache directory (e.g., `~/.cache/cargo-brief/`). Must handle cache invalidation on version changes.

## Common Mistakes
- Extracting `tmp_manifest_str` into a helper function that drops `TempDir` at scope end → borrowed path points to deleted directory. Rust's borrow checker currently catches this, but refactoring to `String` ownership would mask the issue.
- No timeout on `cargo rustdoc` subprocess. Large crates (e.g., `bevy`) can hang for minutes on first build. User must Ctrl-C manually.

## Technical Debt
- No progress indication for downloads/builds of remote crates.
- No caching — each `--crates` invocation re-downloads and re-compiles.
- No feature flag support — crates with conditional `pub` items gated by features show incomplete API without warning.
