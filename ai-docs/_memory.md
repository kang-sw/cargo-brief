# cargo-brief — Cross-Session Memory

<!-- AI-maintained. Update after each non-trivial session. Prune aggressively. -->

## Build & Workflow

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy`
- Requires nightly toolchain for rustdoc JSON generation (`cargo +nightly rustdoc`)

## Recent Work

- **LSP daemon spawn diagnostics**: `spawn_daemon` redirects stderr to `lsp.log` (truncated each start), returns child PID. `wait_for_socket` checks PID liveness each poll iteration — daemon death detected in 50-500ms instead of 120s timeout. Error messages include last 20 lines of daemon log. Cleanup paths (daemon exit, `cmd_stop`) remove `lsp.log`.
- **LSP references command**: `cargo brief lsp references <symbol> [-q]` — first query command. `src/lsp/query.rs` handles symbol resolution via `workspace/symbol`, references via `textDocument/references`. Daemon-side formatting (reads source files, groups by file). Fresh UDS connection per query with 30s timeout. `handle_client` in daemon.rs now accepts `&mut RaTransport` + `&Path` for query forwarding. `DaemonRequest::References` + `DaemonResponse::QueryResult` protocol variants.
- **LSP daemon file watcher**: `src/lsp/watcher.rs` — `notify 6` filesystem watcher sends `workspace/didChangeWatchedFiles` notifications to ra when `.rs`/`Cargo.*` files change. `DebounceBuffer` batches events for 300ms. Integrated into daemon.rs main loop via `mpsc::channel` + `try_recv()`. Graceful degradation if watcher fails. `notify` dep under `[target.'cfg(unix)'.dependencies]`.
- **LSP daemon bootstrap**: `cargo brief lsp {touch,stop,status}` — persistent rust-analyzer daemon per workspace. `src/lsp/` module (mod.rs, daemon.rs, client.rs, protocol.rs, transport.rs, watcher.rs). Synchronous daemon with UDS communication, idle timeout (10min, env-overridable via `CARGO_BRIEF_LSP_TIMEOUT`). Hidden `__lsp-daemon` re-exec entry point in main.rs. FNV-1a hash of canonical workspace root → socket dir. `#[cfg(unix)]` gated. `libc` dep added for stale PID detection. `workspace_root: PathBuf` added to `CargoMetadataInfo`.
- **Code subcommand flexible positionals**: `CodeArgs` uses variadic `args: Vec<String>` (1–3). `resolve_code_args()` dispatches by count + kind-keyword detection. `self` target → all workspace members (not just current package). Named target → single crate. Remote (-C) requires explicit TARGET. Dep sources deduplicated against primary sources.
- **Code subcommand Phase 2**: Three dep modes in `run_code_pipeline()`: default (accessible-path BFS via `discover_accessible_deps()`), `--no-deps` (target only), `--all-deps` (`load_dep_package_dirs()` direct deps, no nightly). `discover_accessible_deps()` is standalone BFS. `load_dep_package_dirs()` in resolve.rs uses `packages[].id` for robust node matching.

## Workspace Reference

- Crate name: `cargo-brief` (binary: `cargo-brief`, lib: `cargo_brief`)
- Entry: `src/lib.rs` → `run_api_pipeline(args, remote)` + `run_search_pipeline(args, remote)` + `run_examples_pipeline(args, remote)` + `run_summary_pipeline(args, remote)` + `run_ts_pipeline(args, remote)` + `run_code_pipeline(args, remote)` + `run_lsp_command(args, remote)` (unix-only), `src/main.rs` → `BriefDirect` parsing, `RemoteOpts` extraction, subcommand dispatch + `__lsp-daemon` early-exit
- LSP query pipeline: query.rs `resolve_symbol()` → `find_references()` → `format_references()`. Reusable for blast-radius/call-hierarchy.
- Pipeline: All pipelines take `(args, &RemoteOpts)`. Build `PipelineContext` (local or remote), then call shared pipeline. Remote branching: `if remote.crates { ... spec from args.target.crate_name ... }`
- CLI types: `ApiArgs`, `SearchArgs`, `ExamplesArgs`, `SummaryArgs`, `TsArgs`, `CodeArgs`, `CleanArgs`, `LspArgs` + shared `TargetArgs`/`FilterArgs`/`GlobalArgs` + `RemoteOpts` (plain struct, not clap). `BriefDirect` has `-C`, `-F`, `--no-cache` as `global = true` flags.
- Modules: `cli`, `code`, `cross_crate`, `examples`, `lsp` (unix-only), `remote`, `resolve`, `rustdoc_json`, `model`, `render`, `search`, `summary`, `ts`
- Test fixture: `test_fixture/` (workspace with `glob-source`/`glob-inner`/`named-source` sub-crates for cross-crate glob and named re-export testing)
- Integration tests: `tests/integration.rs` (224 tests)

## Documented Dependencies

- (none yet — add entries here as API drift is discovered)
