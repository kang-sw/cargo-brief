# Remote Pipeline

## Entry Points
- `src/lib.rs` — `build_remote_context_api()` / `build_remote_context_search()` produce a `PipelineContext`; `run_shared_api_pipeline()` / `run_shared_search_pipeline()` consume it and are shared with the local path.
- `src/remote.rs` — `parse_crate_spec()`, `resolve_workspace()`, `clean_cache()`, `WorkspaceDir`.
- `src/cross_crate.rs` — `resolve_cross_crate_module()`, `build_cross_crate_index()`, `root_has_cross_crate_reexports()`.

## Module Contracts
- `build_remote_context_api()` / `build_remote_context_search()` guarantee: `WorkspaceDir` is stored in `PipelineContext._workspace` to keep it alive for the entire pipeline. Manifest path is an owned `String` — no borrow chain. `PipelineContext.use_cache = true` for all remote contexts. `PipelineContext.available_packages` is populated from Cargo.lock at context-build time via `rustdoc_json::load_lockfile_packages()` — empty if Cargo.lock is absent. The crate spec comes from `args.target.crate_name` (the TARGET positional), not from a dedicated `--crates SPEC` flag — `RemoteOpts.crates` is a boolean mode switch, not a value.
- `pre_warm_cross_crate_json()` runs before `build_cross_crate_index()` / `expand_glob_reexports()` in all three shared pipelines (api/search/summary) when `root_has_cross_crate_reexports` is true. It performs a recursive BFS (max 8 levels) using `batch_generate_rustdoc_json()`, which issues a single `cargo doc -p a -p b ...` invocation per batch level instead of sequential `cargo rustdoc` calls. Failures are silently suppressed; individual generation in `cross_crate.rs` remains as fallback.
- Both remote and local pipelines go through the same `run_shared_api_pipeline()` with three mutually exclusive sub-paths evaluated in order: (1) module-target+cross-crate, (2) recursive+cross-crate, (3) normal. Cross-crate discovery now also runs for local crates that have cross-crate re-exports.
- `generate_rustdoc_json(..., use_cache=true)` skips `cargo rustdoc` if the `.json` file already exists in `target/doc/`. Remote pipelines always set `use_cache=true`. Local pipelines set `use_cache` based on workspace membership: true for non-member deps (e.g. `bevy` from a game project), false for workspace members (source may have changed).
- `run_examples_pipeline` remote path does not call `generate_rustdoc_json` at all — it uses `resolve::find_dep_source_root` to locate the crate source dir on disk, then reads `.rs` files directly. No model is built.
- `resolve_workspace(spec, features, no_cache)` returns `(WorkspaceDir, Option<String>)`. The second element is the resolved exact version string (e.g. `"1.0.200"`). Cached workspaces persist at `cache_dir()/name[version]` (or `name[version]+feat1+feat2` with alpha-sorted features). Cargo reuses build artifacts on subsequent calls. With `no_cache`, version resolution is best-effort and the workspace is a `TempDir`.
- `clean_cache(spec)` with a non-empty spec glob-matches all directories starting with `name[` prefix and also removes `versions/{name}.json`. Empty spec removes all of `cache_dir()`. Prints removed paths and sizes to stderr.
- `parse_rustdoc_json_cached()` reads/writes a `.bin` sidecar file next to the `.json`. If the `.bin` exists and is newer than the `.json`, it deserializes via bincode. A corrupted `.bin` silently falls back to JSON re-parse and overwrites the `.bin`.

## Coupling
- `WorkspaceDir` lifetime → all downstream calls: `manifest_path` is an owned `String` derived from `workspace.path().join("Cargo.toml")`. No borrow chain — all downstream calls receive `&manifest_path`.
- `parse_crate_spec()` version semantics: bare name → `"*"`, `name@version` with fewer than 2 dots → verbatim (e.g., `serde@1` → `"1"`), `name@x.y.z` (2+ dots) → `"=x.y.z"` (exact pin).
- `fetch_resolved_version(name, version_req)` calls the crates.io REST API (`/api/v1/crates/{name}`) and caches the response at `cache_dir()/versions/{name}.json` for 24h. Exact specs (starting with `=`) skip the network entirely. On API failure, stale cache is used with a stderr warning. If no cache exists and the network fails, the call returns an error.
- `build_remote_crate_header()` uses `resolved_version` (from `resolve_workspace`) first, then falls back to reading Cargo.lock via `resolve_crate_version()`. This means the header can show the version before `cargo rustdoc` runs.
- Cache location priority: `$CARGO_BRIEF_CACHE_DIR` > `$XDG_CACHE_HOME/cargo-brief/crates` > `$HOME/.cache/cargo-brief/crates`.
- `cross_crate` resolution uses the same `manifest_path` and `target_dir` as the primary crate. Sub-crate JSON files land in the same `target/doc/` directory — they persist across invocations and are reused when `use_cache=true`.
- `clean` is a first-class `BriefCommand::Clean` subcommand variant dispatched in `main.rs`. It calls `cargo_brief::clean_cache(spec)` directly — it is not a pipeline stage and never builds a `PipelineContext`.

## Extension Points & Change Recipes
- **Add feature flag support for sub-crates**: `-F`/`--features` on `BriefDirect` is already wired into `RemoteOpts.features` and passed to `resolve_workspace`. To propagate features to cross-crate deps, modify `write_workspace_files()` in `remote.rs` to accept per-dep feature lists.
- **Add cache invalidation**: The normalized cache dir (`name[version]`) is already version-pinned — changing the spec produces a new directory. Feature flag changes also produce a new directory (features are encoded in the dir name). No content comparison is needed.
- **Increase cross-crate hop depth**: Change the `for _ in 0..5` loop limit in `follow_use_chain()` in `cross_crate.rs`.

## Common Mistakes
- No timeout on `cargo rustdoc` subprocess. Large crates (e.g., `bevy`) can hang for minutes on first build. User must Ctrl-C manually.
- `generate_rustdoc_json(..., use_cache=true)` only checks if the `.json` file exists — it does not validate the file corresponds to the requested crate version. Manually placing a `.json` file in the cache dir would be returned silently.
- Cross-crate module path not found after >5 re-export hops → `resolve_cross_crate_module` returns `None`, falls through to normal render, which then fails with "module not found". No hint that cross-crate resolution was attempted.
- `batch_generate_rustdoc_json()` sets `RUSTDOCFLAGS` via `cmd.env()` — it silently overwrites any `RUSTDOCFLAGS` the caller's environment already has. This differs from `generate_rustdoc_json()`, which passes flags as trailing `-- ...` arguments to `cargo rustdoc`. The two code paths are not interchangeable. `RUSTDOCFLAGS` in `batch_generate_rustdoc_json()` MUST include `--document-private-items`; omitting it causes all batch-generated JSON to exclude private module contents, making `pub use private_mod::*` glob re-exports silently empty with no error.
- Pre-warming uses crate names from `cross_crate::collect_external_crate_names()`, which are Rust-identifier form (underscores). `normalize_to_lockfile_name()` converts these to the Cargo.lock form (preferring hyphenated if present) before passing to `batch_generate_rustdoc_json()`. If a name matches neither underscore nor hyphenated form in Cargo.lock, it is silently filtered out (not a real package). For crates with multiple versions in Cargo.lock, `normalize_to_lockfile_name()` returns `name@latest_version` (via `LockfilePackages::resolve_spec`) — this version-qualified spec is passed straight to cargo's `-p` flag and is required to avoid "package is ambiguous" errors.
- `generate_rustdoc_json()` auto-retries with a version-qualified spec when cargo reports "is ambiguous", but only in the non-verbose (captured output) path. In verbose mode (`Stdio::inherit()`), cargo's stderr is not captured, so the ambiguity error is never detected. Both `pre_warm_cross_crate_json` (via `normalize_to_lockfile_name`) and `load_or_find_source_crate` (via `resolve_spec`) resolve specs upfront, so the auto-retry path is a fallback for edge cases only (e.g. missing Cargo.lock).

## Technical Debt
- No progress indication for downloads/builds of remote crates or sub-crates during cross-crate discovery.
- Version cache (`versions/{name}.json`) is TTL-based (24h), not event-based. A new release during the TTL window will not be picked up until the cache expires or `cargo brief clean` is used.
- No feature flag support for sub-crates during cross-crate resolution — only the primary crate's features are applied.
- `.bin` sidecar files in `target/doc/` are never cleaned up by `cargo brief clean` (which removes the workspace dir, not the target dir). Stale bincode caches can accumulate indefinitely.
