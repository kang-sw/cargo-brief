# Remote Pipeline

## Entry Points
- `src/lib.rs` — `run_remote_pipeline()` (private helper), `render_remote_normal()` (extracted helper).
- `src/remote.rs` — `parse_crate_spec()`, `resolve_workspace()`, `clean_cache()`, `WorkspaceDir`.
- `src/cross_crate.rs` — `resolve_cross_crate_module()`, `discover_all_reexported_crates()`, `root_has_cross_crate_reexports()`.

## Module Contracts
- `run_remote_pipeline()` guarantees: `WorkspaceDir` is held alive for the entire function scope. Manifest path is an owned `String` — no borrow chain. After building the primary model, `root_has_cross_crate_reexports()` is called once and the result gates all cross-crate branches.
- Remote pipeline uses `document_private_items=true` for both the primary crate and sub-crates resolved via `cross_crate`. This deviates from the old contract in `visibility.md` — private modules are included so facade crate re-export chains remain traversable.
- `run_remote_pipeline()` has four mutually exclusive sub-paths evaluated in order: (1) search+cross-crate, (2) module-target+cross-crate, (3) recursive+cross-crate, (4) normal. Only the first matching path executes.
- `resolve_workspace(spec, no_cache)` returns `WorkspaceDir::Cached(PathBuf)` or `WorkspaceDir::Temp(TempDir)`. Cached workspaces persist at `cache_dir()/sanitize_spec(spec)`. Cargo reuses build artifacts on subsequent calls.
- `clean_cache(spec)` deletes `cache_dir()/sanitize_spec(spec)` (specific) or all of `cache_dir()` (empty spec). Prints removed path and size to stderr.
- `generate_rustdoc_json_cached()` skips `cargo rustdoc` if the `.json` file already exists in `target/doc/`. Only safe for remote pipelines where versions are locked.
- `parse_rustdoc_json_cached()` reads/writes a `.bin` sidecar file next to the `.json`. If the `.bin` exists and is newer than the `.json`, it deserializes via bincode. A corrupted `.bin` silently falls back to JSON re-parse and overwrites the `.bin`.

## Coupling
- `WorkspaceDir` lifetime → all downstream calls: `manifest_path` is an owned `String` derived from `workspace.path().join("Cargo.toml")`. No borrow chain — all downstream calls receive `&manifest_path`.
- `parse_crate_spec()` version semantics: bare name → `"*"`, `name@version` with fewer than 2 dots → verbatim (e.g., `serde@1` → `"1"`), `name@x.y.z` (2+ dots) → `"=x.y.z"` (exact pin).
- Cache location priority: `$CARGO_BRIEF_CACHE_DIR` > `$XDG_CACHE_HOME/cargo-brief/crates` > `$HOME/.cache/cargo-brief/crates`.
- `cross_crate` resolution uses the same `manifest_path` and `target_dir` as the primary crate. Sub-crate JSON files land in the same `target/doc/` directory — they persist across invocations and are reused by `generate_rustdoc_json_cached`.
- `--clean` is handled in `main.rs` before `run_pipeline()` is called — it is an early exit, not a pipeline stage.

## Extension Points & Change Recipes
- **Add feature flag support**: Modify `write_workspace_files()` in `remote.rs` to include `features = [...]` in the generated Cargo.toml. Add `--features` flag to `BriefArgs` (already present — wire it through if not already).
- **Add cache invalidation**: Compare stored Cargo.toml content with generated content in `resolve_workspace()`. If different, overwrite and let Cargo handle the rebuild.
- **Increase cross-crate hop depth**: Change the `for _ in 0..5` loop limit in `follow_use_chain()` and `resolve_single_reexport()` in `cross_crate.rs`.

## Common Mistakes
- No timeout on `cargo rustdoc` subprocess. Large crates (e.g., `bevy`) can hang for minutes on first build. User must Ctrl-C manually.
- `generate_rustdoc_json_cached()` only checks if the `.json` file exists — it does not validate that the file corresponds to the requested crate version. If a cached `.json` was produced by a different version (e.g., after manually editing `Cargo.toml` in the cache dir), stale data is returned silently.
- Cross-crate module path not found after >5 re-export hops → `resolve_cross_crate_module` returns `None`, falls through to normal render, which then fails with "module not found". No hint that cross-crate resolution was attempted.

## Technical Debt
- No progress indication for downloads/builds of remote crates or sub-crates during cross-crate discovery.
- No automatic cache invalidation — wildcard specs keep their Cargo.lock resolution. Use `--no-cache` to force refresh.
- No feature flag support for sub-crates during cross-crate resolution — only the primary crate's features are applied.
- `.bin` sidecar files in `target/doc/` are never cleaned up by `--clean` (which removes the workspace dir, not the target dir). Stale bincode caches can accumulate indefinitely.
