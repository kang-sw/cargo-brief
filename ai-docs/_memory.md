# cargo-brief — Cross-Session Memory

<!-- AI-maintained. Update after each non-trivial session. Prune aggressively. -->

## Build & Workflow

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy`
- Requires nightly toolchain for rustdoc JSON generation (`cargo +nightly rustdoc`)

## Recent Work

- **LSP FIFO IPC refactor**: Replaced Unix domain socket (UDS) IPC with FIFO pair (`lsp.req` + `lsp.resp`) + `flock` serialization. Eliminates socket syscalls blocked by Claude Code's macOS sandbox. Daemon opens FIFOs with `O_RDWR` (prevents POLLHUP races), creates FIFOs after ra init (readiness invariant). `ensure_daemon()` returns `PathBuf` (daemon dir), `send_command()` takes `(daemon_dir, request, timeout)`. `handle_client()` → `handle_request()` (takes `&DaemonRequest`, returns `DaemonResponse`). `DaemonRequest::Ping` removed (PID + FIFO existence replaces ping). EINTR-safe `poll_retry()` helper. Stale FIFO data drained before reading responses.
- **LSP blast-radius + call-hierarchy commands**: `cargo brief lsp blast-radius <symbol> [--depth N] [-q]` shows direct + transitive callers via BFS on LSP `callHierarchy/incomingCalls`. `cargo brief lsp call-hierarchy <symbol> [--outgoing] [-q]` shows incoming/outgoing call tree. Both use `callHierarchy/prepare` → `incomingCalls`/`outgoingCalls`. `serde_json::Value` for CallHierarchyItem (preserves opaque `data` field). BFS dedup via `HashSet<(uri, selectionRange.start.line)>`, depth clamped 1..=10. `DaemonRequest::BlastRadius` + `DaemonRequest::CallHierarchy` protocol variants.
- **LSP references command**: `cargo brief lsp references <symbol> [-q]` — first query command. `src/lsp/query.rs` handles symbol resolution via `workspace/symbol`, references via `textDocument/references`. Daemon-side formatting (reads source files, groups by file). `handle_request` in daemon.rs accepts `&mut RaTransport` + `&Path` for query forwarding. `DaemonRequest::References` + `DaemonResponse::QueryResult` protocol variants.
- **LSP daemon bootstrap**: `cargo brief lsp {touch,stop,status}` — persistent rust-analyzer daemon per workspace. `src/lsp/` module (mod.rs, daemon.rs, client.rs, protocol.rs, transport.rs, watcher.rs). Synchronous daemon with FIFO IPC, idle timeout (10min, env-overridable via `CARGO_BRIEF_LSP_TIMEOUT`). Hidden `__lsp-daemon` re-exec entry point in main.rs. FNV-1a hash of canonical workspace root → daemon dir in `<target>/cargo-brief-lsp/<hash>`. `#[cfg(unix)]` gated. `libc` dep for PID detection, FIFO creation, flock, poll.
- **Code subcommand flexible positionals**: `CodeArgs` uses variadic `args: Vec<String>` (1–3). `resolve_code_args()` dispatches by count + kind-keyword detection. `self` target → all workspace members (not just current package). Named target → single crate. Remote (-C) requires explicit TARGET. Dep sources deduplicated against primary sources.
- **Code subcommand Phase 2**: Three dep modes in `run_code_pipeline()`: default (accessible-path BFS via `discover_accessible_deps()`), `--no-deps` (target only), `--all-deps` (`load_dep_package_dirs()` direct deps, no nightly). `discover_accessible_deps()` is standalone BFS. `load_dep_package_dirs()` in resolve.rs uses `packages[].id` for robust node matching.

## Workspace Reference

- Crate name: `cargo-brief` (binary: `cargo-brief`, lib: `cargo_brief`)
- Entry: `src/lib.rs` → `run_api_pipeline(args, remote)` + `run_search_pipeline(args, remote)` + `run_examples_pipeline(args, remote)` + `run_summary_pipeline(args, remote)` + `run_ts_pipeline(args, remote)` + `run_code_pipeline(args, remote)` + `run_lsp_command(args, remote)` (unix-only), `src/main.rs` → `BriefDirect` parsing, `RemoteOpts` extraction, subcommand dispatch + `__lsp-daemon` early-exit
- LSP query pipeline: query.rs `resolve_symbol()` shared across all query commands. References: `find_references()` → `format_references()`. Call hierarchy: `prepare_call_hierarchy()` → `incoming_calls()`/`outgoing_calls()` → `format_call_hierarchy()`/`format_blast_radius()`.
- Pipeline: All pipelines take `(args, &RemoteOpts)`. Build `PipelineContext` (local or remote), then call shared pipeline. Remote branching: `if remote.crates { ... spec from args.target.crate_name ... }`
- CLI types: `ApiArgs`, `SearchArgs`, `ExamplesArgs`, `SummaryArgs`, `TsArgs`, `CodeArgs`, `CleanArgs`, `LspArgs` + shared `TargetArgs`/`FilterArgs`/`GlobalArgs` + `RemoteOpts` (plain struct, not clap). `BriefDirect` has `-C`, `-F`, `--no-cache` as `global = true` flags.
- Modules: `cli`, `code`, `cross_crate`, `examples`, `lsp` (unix-only), `remote`, `resolve`, `rustdoc_json`, `model`, `render`, `search`, `summary`, `ts`
- Test fixture: `test_fixture/` (workspace with `glob-source`/`glob-inner`/`named-source` sub-crates for cross-crate glob and named re-export testing)
- Integration tests: `tests/integration.rs` (224 tests)

## Documented Dependencies

- (none yet — add entries here as API drift is discovered)
