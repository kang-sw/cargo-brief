# cargo-brief — Cross-Session Memory

<!-- AI-maintained. Update after each non-trivial session. Prune aggressively. -->

## Build & Workflow

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy`
- Requires nightly toolchain for rustdoc JSON generation (`cargo +nightly rustdoc`)

## Active Work

- **LSP cross-platform IPC refactoring** (`260328-refactor-lsp-cross-platform-ipc`):
  Extracting platform-specific IPC and process management into abstraction modules.
  Unix keeps FIFOs; Windows uses atomic-rename file protocol (sandbox-safe, target/ only).
  **Phase 1 & 2 complete**: `src/lsp/ipc/{mod,unix,windows}.rs` (IPC) and
  `src/lsp/process/{mod,unix,windows}.rs` (process mgmt) extracted from client.rs/daemon.rs.
  `windows-sys` 0.59 with FileSystem/IO features. client.rs now daemon lifecycle only.
  daemon.rs uses `DaemonIpc` struct. `poll_retry` re-exported from ipc for ra stdout.
  Remaining: Phase 3 (cfg gate removal + transport abstraction + CI).
  Cross-compilation check blocked by missing MSVC C headers (ring/tree-sitter build scripts).
  Windows runtime testing deferred to `260326-feat-lsp-windows-support`.

## Recent Work

- **LSP indexing status tracking**: Daemon tracks ra's `$/progress` begin/end notifications to determine indexing state. `RaStatus::Indexing` variant added. Main loop drains ra stdout via poll-then-read pattern (`drain_ra_messages()`). Query commands gate on `wait_for_ready()` (60s default, `CARGO_BRIEF_LSP_READY_TIMEOUT` env var). Client-side query timeout increased to 120s. `send_request_and_wait()` replies to server-initiated requests. `window.workDoneProgress: true` declared in capabilities. Fallback: no `$/progress` ever + uptime > 10s → assume Ready.
- **LSP FIFO IPC refactor**: Replaced UDS IPC with FIFO pair + `flock` serialization for macOS sandbox compatibility.
- **LSP blast-radius + call-hierarchy commands**: BFS incoming/outgoing callers via `callHierarchy` LSP methods.
- **LSP references command**: First query command; symbol resolution via `workspace/symbol`.
- **LSP daemon bootstrap**: Persistent ra daemon per workspace, FIFO IPC, idle timeout.
- **Code subcommand Phase 2**: Three dep modes (default BFS, `--no-deps`, `--all-deps`).

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
