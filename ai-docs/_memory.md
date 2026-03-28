# cargo-brief — Cross-Session Memory

<!-- AI-maintained. Update after each non-trivial session. Prune aggressively. -->

## Build & Workflow

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy`
- Requires nightly toolchain for rustdoc JSON generation (`cargo +nightly rustdoc`)

## Active Work

- **LSP cross-platform IPC refactoring** (`260328-refactor-lsp-cross-platform-ipc`):
  All 3 phases complete. `lsp` module is now cross-platform (no `#[cfg(unix)]` gate).
  Phase 1: IPC abstraction (`src/lsp/ipc/{mod,unix,windows}.rs`).
  Phase 2: Process management abstraction (`src/lsp/process/{mod,unix,windows}.rs`).
  Phase 3: Background reader thread replaces `libc::poll` in transport.rs; `libc` gated
  to `cfg(unix)`, `notify` made unconditional. `shutdown_ra()` simplified to fire-and-forget.
  Windows runtime testing deferred to `260326-feat-lsp-windows-support`.

## Recent Work

- **LSP indexing status tracking**: Daemon tracks ra's `$/progress` begin/end notifications to determine indexing state. `RaStatus::Indexing` variant added. Main loop drains ra stdout via poll-then-read pattern (`drain_ra_messages()`). Query commands gate on `wait_for_ready()` (60s default, `CARGO_BRIEF_LSP_READY_TIMEOUT` env var). Client-side query timeout increased to 120s. `send_request_and_wait()` replies to server-initiated requests. `window.workDoneProgress: true` declared in capabilities. Fallback: no `$/progress` ever + uptime > 10s → assume Ready.
- **LSP FIFO IPC refactor**: Replaced UDS IPC with FIFO pair + `flock` serialization for macOS sandbox compatibility.
- **LSP blast-radius + call-hierarchy commands**: BFS incoming/outgoing callers via `callHierarchy` LSP methods.
- **LSP grep-based fallback resolution**: `resolve_symbol()` now has two-stage resolution: workspace/symbol (fast, workspace items) → grep+definition fallback (slower, finds external deps). `workspace_root: &Path` parameter added to `resolve_symbol()`. All 3 query commands benefit automatically.
- **LSP references command**: First query command; symbol resolution via `workspace/symbol`.
- **LSP daemon bootstrap**: Persistent ra daemon per workspace, FIFO IPC, idle timeout.
- **Code subcommand Phase 2**: Three dep modes (default BFS, `--no-deps`, `--all-deps`).

## Workspace Reference

- Crate name: `cargo-brief` (binary: `cargo-brief`, lib: `cargo_brief`)
- Entry: `src/lib.rs` → `run_api_pipeline(args, remote)` + `run_search_pipeline(args, remote)` + `run_examples_pipeline(args, remote)` + `run_summary_pipeline(args, remote)` + `run_ts_pipeline(args, remote)` + `run_code_pipeline(args, remote)` + `run_lsp_command(args, remote)`, `src/main.rs` → `BriefDirect` parsing, `RemoteOpts` extraction, subcommand dispatch + `__lsp-daemon` early-exit
- LSP query pipeline: query.rs `resolve_symbol(transport, query, workspace_root)` shared across all query commands. Two-stage: workspace/symbol first, then grep+definition fallback for external deps. References: `find_references()` → `format_references()`. Call hierarchy: `prepare_call_hierarchy()` → `incoming_calls()`/`outgoing_calls()` → `format_call_hierarchy()`/`format_blast_radius()`.
- Pipeline: All pipelines take `(args, &RemoteOpts)`. Build `PipelineContext` (local or remote), then call shared pipeline. Remote branching: `if remote.crates { ... spec from args.target.crate_name ... }`
- CLI types: `ApiArgs`, `SearchArgs`, `ExamplesArgs`, `SummaryArgs`, `TsArgs`, `CodeArgs`, `CleanArgs`, `LspArgs` + shared `TargetArgs`/`FilterArgs`/`GlobalArgs` + `RemoteOpts` (plain struct, not clap). `BriefDirect` has `-C`, `-F`, `--no-cache` as `global = true` flags.
- Modules: `cli`, `code`, `cross_crate`, `examples`, `lsp`, `remote`, `resolve`, `rustdoc_json`, `model`, `render`, `search`, `summary`, `ts`
- Test fixture: `test_fixture/` (workspace with `glob-source`/`glob-inner`/`named-source` sub-crates for cross-crate glob and named re-export testing)
- Integration tests: `tests/integration.rs` (224 tests)

## Documented Dependencies

- (none yet — add entries here as API drift is discovered)
