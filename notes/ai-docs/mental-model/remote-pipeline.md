# Remote Pipeline

## Entry Points
- `src/lib.rs:100-142` — `run_remote_pipeline()` (private helper).
- `src/remote.rs` — `parse_crate_spec()`, `resolve_workspace()`, `WorkspaceDir`.

## Module Contracts
- `run_remote_pipeline()` guarantees: `WorkspaceDir` is held alive for the entire function scope. Manifest path is an owned `String` — no borrow chain.
- Remote pipeline always uses `document_private_items=false` and `same_crate=false`. These two flags MUST stay in sync — see `visibility.md`.
- `resolve_workspace(spec, no_cache)` returns `WorkspaceDir::Cached(PathBuf)` or `WorkspaceDir::Temp(TempDir)`. Cached workspaces persist at `cache_dir()/sanitize_spec(spec)`. Cargo reuses build artifacts on subsequent calls.

## Coupling
- `WorkspaceDir` lifetime → all downstream calls: `manifest_path` is an owned `String` derived from `workspace.path().join("Cargo.toml")`. No borrow chain — all downstream calls receive `&manifest_path`.
- `parse_crate_spec()` version semantics: bare name → `"*"`, `name@version` with fewer than 2 dots → verbatim (e.g., `serde@1` → `"1"`), `name@x.y.z` (2+ dots) → `"=x.y.z"` (exact pin).
- Cache location priority: `$CARGO_BRIEF_CACHE_DIR` > `$XDG_CACHE_HOME/cargo-brief/crates` > `$HOME/.cache/cargo-brief/crates`.

## Extension Points & Change Recipes
- **Add feature flag support**: Modify `write_workspace_files()` to include `features = [...]` in the generated Cargo.toml. Add `--features` flag to `BriefArgs`.
- **Add cache invalidation**: Compare stored Cargo.toml content with generated content. If different, overwrite and let Cargo handle the rebuild.

## Common Mistakes
- No timeout on `cargo rustdoc` subprocess. Large crates (e.g., `bevy`) can hang for minutes on first build. User must Ctrl-C manually.

## Technical Debt
- No progress indication for downloads/builds of remote crates.
- No automatic cache invalidation — wildcard specs keep their Cargo.lock resolution. Use `--no-cache` to force refresh.
- No feature flag support — crates with conditional `pub` items gated by features show incomplete API without warning.
