# cargo-brief — Cross-Session Memory

<!-- AI-maintained. Update after each non-trivial session. Prune aggressively. -->

## Build & Workflow

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy`
- Requires nightly toolchain for rustdoc JSON generation (`cargo +nightly rustdoc`)

## Recent Work

- **Code subcommand flexible positionals**: `CodeArgs` uses variadic `args: Vec<String>` (1–3). `resolve_code_args()` dispatches by count + kind-keyword detection. `self` target → all workspace members (not just current package). Named target → single crate. Remote (-C) requires explicit TARGET. Dep sources deduplicated against primary sources.
- **Code subcommand Phase 2**: Three dep modes in `run_code_pipeline()`: default (accessible-path BFS via `discover_accessible_deps()`), `--no-deps` (target only), `--all-deps` (`load_dep_package_dirs()` direct deps, no nightly). `discover_accessible_deps()` is standalone BFS. `load_dep_package_dirs()` in resolve.rs uses `packages[].id` for robust node matching.
- **Code subcommand Phase 1**: `src/code.rs` — ItemKind enum, pre-crafted tree-sitter queries, smart-case matching, grep pre-filter, module/parent context, limit/offset, quiet mode. `code::search_code(&sources, name, kind, args)` takes vec of (name, source_root) pairs.
- **Named re-export expansion**: `expand_glob_reexports()` second pass detects non-glob cross-crate `Use` items. `render::render_single_inlined_item()` renders a single named item from source models. `apply_glob_expansions()` replaces `pub use {source};` lines. `GlobExpansionResult.named_reexports` field. Phase 2 glob loop iterates `item_names.keys()` (not `source_models`) to avoid `seen_names` poisoning. Module re-exports preserved. `--no-expand-glob` suppresses both.
- **Search member display**: `--members` flag on `SearchArgs`. Default: member items (fields, variants, methods, assoc items) suppressed unless search token exactly matches member name. `--members` expands all members of matched types. Collapsed display: `-::member` continuation lines. `is_member()` distinguishes members from free items. Cross-crate search walks struct fields + enum variants + union fields. `render_search_filtered` and `search_cross_crate_index` have `members: bool` param.

## Workspace Reference

- Crate name: `cargo-brief` (binary: `cargo-brief`, lib: `cargo_brief`)
- Entry: `src/lib.rs` → `run_api_pipeline(args, remote)` + `run_search_pipeline(args, remote)` + `run_examples_pipeline(args, remote)` + `run_summary_pipeline(args, remote)` + `run_ts_pipeline(args, remote)` + `run_code_pipeline(args, remote)`, `src/main.rs` → `BriefDirect` parsing, `RemoteOpts` extraction, subcommand dispatch
- Pipeline: All pipelines take `(args, &RemoteOpts)`. Build `PipelineContext` (local or remote), then call shared pipeline. Remote branching: `if remote.crates { ... spec from args.target.crate_name ... }`
- CLI types: `ApiArgs`, `SearchArgs`, `ExamplesArgs`, `SummaryArgs`, `TsArgs`, `CodeArgs`, `CleanArgs` + shared `TargetArgs`/`FilterArgs`/`GlobalArgs` + `RemoteOpts` (plain struct, not clap). `BriefDirect` has `-C`, `-F`, `--no-cache` as `global = true` flags.
- Modules: `cli`, `code`, `cross_crate`, `examples`, `remote`, `resolve`, `rustdoc_json`, `model`, `render`, `search`, `summary`, `ts`
- Test fixture: `test_fixture/` (workspace with `glob-source`/`glob-inner`/`named-source` sub-crates for cross-crate glob and named re-export testing)
- Integration tests: `tests/integration.rs` (224 tests)

## Documented Dependencies

- (none yet — add entries here as API drift is discovered)
