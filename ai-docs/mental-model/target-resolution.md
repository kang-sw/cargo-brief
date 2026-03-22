# Target Resolution

## Entry Points
- `src/resolve.rs` — `resolve_target()` (4-case algorithm), `load_cargo_metadata()`.
- `src/main.rs` — dual invocation dispatch (`cargo brief` vs `cargo-brief`).

## Module Contracts
- `resolve_target()` guarantees: given CLI args and metadata, returns a `ResolvedTarget` with `package_name` (always a package name, never a module) and `module_path` (optional). For workspace package matches, `find_workspace_package()` returns the actual workspace package name (e.g., `my-crate`), not the user's raw input (e.g., `my_crate`).
- `load_cargo_metadata()` guarantees: single `cargo metadata --no-deps` call; `current_package` is detected by matching cwd against package manifest directories. `package_manifest_dirs` is populated for every workspace package and is the source of truth for local `examples` path resolution.
- `find_dep_source_root(manifest_path, crate_name)` runs a second `cargo metadata` call (without `--no-deps`) to locate a dependency's source directory. It is only called by `run_examples_pipeline` for the remote path. It is not called in the `api` or `search` pipelines.

## Coupling
- `ResolvedTarget.package_name` ↔ `CrateModel.crate_name()`: These use different naming conventions. Package names use hyphens (`my-crate`), crate names use underscores (`my_crate`). `lib.rs:68` normalizes with `replace('-', "_")` for `same_crate` detection, but other comparisons may not normalize.
- `-C`/`--crates` bypass: When `remote.crates` is `true`, the entire resolve pipeline is skipped and `args.target.crate_name` is used directly as the remote crate spec. The local resolve path is never entered.
- File path detection: `is_file_path()` triggers on `/` or `.rs` suffix. False positives possible for crate names containing `.rs` (unlikely but not validated).

## Extension Points & Change Recipes
- **Add a new resolution case**: Add to the match chain in `resolve_target()`. Cases are evaluated in priority order: `"self"` → contains `"::"` → two-arg → single-arg fallback. New cases must slot into this chain.
- **Change `self` detection**: Modify `load_cargo_metadata()`. The `current_package` field is set by matching cwd against manifest directories. Virtual workspace roots produce `current_package: None`.

## Common Mistakes
- Running `cargo brief -C api serde` and also specifying a local manifest path: `remote.crates = true` bypasses all local resolution; the manifest path is unused. No error or warning.
- Running from virtual workspace root without `--at-package`: `current_package` is `None`, `same_crate` becomes unconditionally `false`. All `pub(crate)` items hidden.
- File path resolution uses three fallbacks: (1) cwd-relative, (2) package `src/`-relative, (3) package-root-relative. If a file exists at multiple locations, the first match wins — potentially resolving to the wrong module.
- `find_dep_source_root` issues a second subprocess (`cargo metadata` with deps). If the remote workspace's dependencies have not been fetched yet (e.g., immediately after `resolve_workspace`), this call may trigger a network download or silently return a stale path on failure.

## Technical Debt
- For non-workspace-match cases (e.g., external crate names in Case 4 fallback), `resolve_target()` returns the raw user input as `package_name`. If the user passes an underscore variant for an external crate, `same_crate` detection at `lib.rs:68` handles it via `replace('-', "_")`, but other downstream comparisons may not normalize.
