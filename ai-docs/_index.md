<!-- Memory policy: prune aggressively as project advances. Completed
     work belongs in git history, not here. Keep only what an AI session
     needs to orient itself and pick up work. If it's derivable from
     code or git log, delete it from this file. -->

# cargo-brief — Project State & Architecture

## What This Tool Is

`cargo-brief` is a Cargo subcommand that extracts and formats a Rust crate's API surface as
pseudo-Rust documentation. It is a visibility-aware, context-sensitive extension of
`cargo-public-api`.

Primary consumer: **AI coding agents** (e.g., Claude Code) that need to understand a crate's
interface without reading full source files — saving context window tokens.

Output style: "text-document-like" pseudo-Rust (not machine-readable JSON). Function bodies
are replaced with `;`, doc comments are preserved verbatim, private items are filtered by
perspective.

---

## Core Concept: Visibility-Aware Perspective

The tool's key differentiator from `cargo-public-api` is **`--at-mod`**: rather than dumping
everything that is technically `pub`, it shows only what would compile if `use`d from the
specified module. This includes re-exports and respects `pub(crate)`, `pub(super)`,
`pub(in path)` appropriately.

For **external crates**, `--at-mod` degenerates to "show `pub` items only" (since cross-crate
visibility is always `pub`-only). This makes external dep support architecturally simpler.

---

## CLI Quick Reference

```
cargo brief [-C] api [target] [module_path] [OPTIONS]
cargo brief [-C] search [target] <pattern> [OPTIONS]
cargo brief [-C] examples [target] [pattern] [OPTIONS]
cargo brief [-C] summary [target] [module_path] [OPTIONS]
cargo brief [-C] ts [target] '<query>' [OPTIONS]
cargo brief [-C] code [target] [kind] <name> [OPTIONS]
cargo brief clean [SPEC]
cargo brief lsp {touch|stop|status|references|blast-radius|call-hierarchy} [OPTIONS]
```

See `ai-docs/spec/cli-surface.md` for full subcommand details, flags, and target resolution rules.

---

## Source Layout

```
src/
  lib.rs           — re-exports all modules, pipeline orchestration via PipelineContext → shared api/search functions
  examples.rs      — example/test/bench file scanning, list mode and grep mode rendering
  main.rs          — CLI arg parsing, subcommand dispatch, RemoteOpts extraction from BriefDirect, __lsp-daemon early-exit
  cli.rs           — Subcommand types: ApiArgs, SearchArgs, ExamplesArgs, SummaryArgs, CleanArgs, LspArgs + shared TargetArgs/FilterArgs/GlobalArgs + RemoteOpts (plain struct)
  cross_crate.rs   — cross-crate module following for facade crates
  lsp/             — LSP daemon management (cross-platform): mod.rs (entry), daemon.rs (main loop), client.rs (daemon lifecycle: ensure/spawn/wait), ipc/ (platform-abstracted IPC: unix.rs FIFO + windows.rs atomic-rename), process/ (platform-abstracted process mgmt: unix.rs + windows.rs), protocol.rs (message framing), transport.rs (LSP JSON-RPC framing + background reader thread), watcher.rs (filesystem watching), query.rs (symbol resolution with grep+definition fallback + references)
  remote.rs        — temp workspace creation for --crates (crates.io fetch) + cache management
  resolve.rs       — flexible target resolution (self, crate::module, fallback) + cargo metadata
  rustdoc_json.rs  — JSON generation (with use_cache param) + parsing (bincode-cached)
  model.rs         — CrateModel with module index, visibility resolution
  render.rs        — pseudo-Rust rendering of all item types
  search.rs        — search mode: leaf item walker + one-line-per-item renderer
  ts.rs            — tree-sitter structural query execution + output formatting
```

### Backend: rustdoc JSON
`cargo +nightly rustdoc -p <crate> -- --output-format json -Z unstable-options --document-private-items`

Parsed via `rustdoc-types` 0.57. Post-macro-expansion output.

### Dependencies
`clap` 4, `rustdoc-types` 0.57, `serde_json` 1, `anyhow` 1, `tempfile` 3, `bincode` 1,
`semver` 1, `ureq` 2, `tree-sitter` 0.25, `tree-sitter-rust` 0.23, `streaming-iterator` 0.1,
`libc` 0.2 (unix-only), `notify` 6.

## Mental Model Documents

Top-level index: `ai-docs/mental-model.md` (pipeline architecture, module contracts, coupling, extension points).

Domain docs in `ai-docs/mental-model/`:

| Document | Domain |
|----------|--------|
| `visibility.md` | Visibility resolution: `is_visible_from`, `same_crate` inference, observer normalization |
| `target-resolution.md` | CLI → package/module resolution: 4-case algorithm, dual invocation, `--crates` bypass |
| `remote-pipeline.md` | `--crates` lifecycle: TempDir borrow chain, version semantics, remote-only constraints |
| `glob-expansion.md` | Glob re-export expansion: string-based detection, Phase 1/2 inlining, coupling with render |
| `search.md` | Pattern parsing, leaf item walker, one-line-per-item renderer, cross-crate index search |
| `testing.md` | Test infrastructure: BriefArgs coupling, fixture contracts, visibility test patterns |
| `lsp-daemon.md` | LSP daemon: re-exec contract, FIFO IPC, flock serialization, idle timeout |

---

## Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary backend | rustdoc JSON + `rustdoc-types` | Post-macro-expansion, official, matches LSP-level output |
| `--at-mod` semantics | "compiles when `use`d from here" | Matches developer mental model; includes re-exports |
| Output format | Pseudo-Rust text (not JSON) | LLM consumption; readable as documentation |
| Item-kind filtering | Default=show all common; `--no-*` to exclude; `--all` adds blanket/auto-trait impls | Subtractive model is more ergonomic |
| Statics grouped with constants | `--no-constants` hides both | Conceptually similar; avoids flag proliferation |
| lib.rs + slim main.rs | `run_api_pipeline()` / `run_search_pipeline()` returns String | Enables integration tests without subprocess |
| External deps | Phase 2 | Adds ~30% complexity; architecture supports it cleanly |
| Target resolution | Semantic layer between CLI and pipeline | `BriefArgs` stays unchanged; resolution in `src/resolve.rs` |
| Single cargo metadata call | `resolve::load_cargo_metadata()` | Eliminates redundant `find_target_dir()` call |
| Ambiguous single arg | Always package | Bare name = package; `self::mod` for own modules |
| File path as module | Heuristic: `/` or `.rs` → file path | 2-level fallback: cwd-relative, then package `src/`-relative |

---

## Build & Test

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy`
- Requires nightly toolchain for rustdoc JSON generation (`cargo +nightly rustdoc`)

## Spec Index

| Spec | Summary |
|------|---------|
| `cli-surface.md` | Subcommands, flags, target resolution, shared option groups |
| `visibility.md` | `--at-mod`/`--at-package` semantics, visibility levels, re-export interaction |
| `output-format.md` | Pseudo-Rust rendering, density controls, item-type display, search/summary format |
| `remote-crates.md` | `-C` flag, crate specs, version resolution, cache management, cross-crate paths |
| `lsp.md` | Daemon lifecycle, query commands (references, blast-radius, call-hierarchy) |

## Conventions

- **Tickets**: `ai-docs/tickets/<status>/YYMMDD-<category>-<name>.md`. Categories:
  `bug`, `feat`, `refactor`, `chore`, `research`. `YYMMDD` is creation date (never changes).
- **Plans**: `ai-docs/.plans/YYYY-MM/DD-hhmm.<name>.md`.
- **Dependency docs**: use `ws-ask-api` / `ai-docs/.deps/`; legacy dependency notes live under `ai-docs/ref/` if present.

## Workspace Reference

- Crate name: `cargo-brief` (binary: `cargo-brief`, lib: `cargo_brief`)
- Entry: `src/lib.rs` → `run_api_pipeline(args, remote)` + `run_search_pipeline(args, remote)` + `run_examples_pipeline(args, remote)` + `run_summary_pipeline(args, remote)` + `run_ts_pipeline(args, remote)` + `run_code_pipeline(args, remote)` + `run_lsp_command(args, remote)`, `src/main.rs` → `BriefDirect` parsing, `RemoteOpts` extraction, subcommand dispatch + `__lsp-daemon` early-exit
- Pipeline: All pipelines take `(args, &RemoteOpts)`. Build `PipelineContext` (local or remote), then call shared pipeline. Remote branching: `if remote.crates { ... spec from args.target.crate_name ... }`
- CLI types: `ApiArgs`, `SearchArgs`, `ExamplesArgs`, `SummaryArgs`, `TsArgs`, `CodeArgs`, `CleanArgs`, `LspArgs` + shared `TargetArgs`/`FilterArgs`/`GlobalArgs` + `RemoteOpts` (plain struct, not clap)
- Modules: `cli`, `code`, `cross_crate`, `examples`, `lsp`, `remote`, `resolve`, `rustdoc_json`, `model`, `render`, `search`, `summary`, `ts`
- Test fixture: `test_fixture/` (workspace with `glob-source`/`glob-inner`/`named-source` sub-crates for cross-crate glob and named re-export testing; also contains `facade_inner`/`facade_pub`/`facade_empty`/`facade_alias` fixtures for named Use→Module re-export through private parents; `proc-macro-fixture` for proc-macro surfacing — one bang, one attribute, one derive macro)
- Integration tests: `tests/integration.rs` (251 tests)

## Documented Dependencies

(none yet — add entries here as API drift is discovered)

## Ticket Queue

1. `260423-bug-cfg-parse-implicit-and-fallback`
2. `260423-feat-subcommand-quickguide-tables`
3. `260423-feat-verbose-download-progress`
4. `260505-bug-rustdoc-lib-target-json-lookup` - harden PR #4 lib-target rustdoc JSON lookup with tests and bounded metadata fallback cost.
5. `260505-feat-search-signature-type-filters` - complete PR #5 search signature type filters with specs, parity tests, README, and changelog updates.

## Session Notes

<!-- Cross-session intent only, 2-5 lines max, delete when stale. -->

No active items.
