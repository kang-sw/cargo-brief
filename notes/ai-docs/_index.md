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

## CLI Interface

```
cargo brief <target> [module_path] [OPTIONS]
```

### Positional Arguments — Flexible Resolution
| Syntax              | Resolves to                                     |
|---------------------|-------------------------------------------------|
| `<crate_name>`      | Named crate (exact match or hyphen/underscore)  |
| `self`              | Current package (cwd-based detection)           |
| `self::module`      | Current package, specific module                |
| `crate::module`     | Named crate, specific module (single-arg)       |
| `<unknown_name>`    | If not a workspace package → self module        |
| `<pkg> <module>`    | Two-arg backward compat: package + module       |
| `src/cli.rs`        | File path → auto-converted to module path       |
| `self src/foo.rs`   | Self package + file path as module              |

### Options
| Flag                    | Description                                                    |
|-------------------------|----------------------------------------------------------------|
| `--at-package <pkg>`    | Caller's package (for visibility resolution)                   |
| `--at-mod <mod_path>`   | Caller's module (determines what is visible)                   |
| `--depth <n>`           | How many submodule levels to recurse into (default: 1)         |
| `--recursive`           | Recurse into all submodules (no depth limit)                   |
| `--all`                 | Show all item kinds including blanket/auto-trait impls          |
| `--no-structs`          | Exclude structs                                                |
| `--no-enums`            | Exclude enums                                                  |
| `--no-traits`           | Exclude traits                                                 |
| `--no-functions`        | Exclude free functions                                         |
| `--no-aliases`          | Exclude type aliases                                           |
| `--no-constants`        | Exclude constants and statics                                  |
| `--no-unions`           | Exclude unions                                                 |
| `--no-macros`           | Exclude macros                                                 |
| `--toolchain <name>`    | Nightly toolchain name (default: `nightly`)                    |
| `--manifest-path <path>`| Path to Cargo.toml                                            |

---

## Source Layout

```
src/
  lib.rs           — re-exports all modules, run_pipeline() entry point
  main.rs          — CLI arg parsing, calls run_pipeline(), prints output
  cli.rs           — BriefArgs struct (clap derive)
  resolve.rs       — flexible target resolution (self, crate::module, fallback) + cargo metadata
  rustdoc_json.rs  — JSON generation and parsing (accepts target_dir from resolve)
  model.rs         — CrateModel with module index, visibility resolution
  render.rs        — pseudo-Rust rendering of all item types
```

### Supported Item Types
Structs (unit, tuple, plain), enums (plain, tuple, struct variants), traits,
free functions (async, const, unsafe), type aliases, constants, statics
(static, static mut), unions, macros (macro_rules!), re-exports (use),
inherent impls, trait impls.

### Backend: rustdoc JSON
`cargo +nightly rustdoc -p <crate> -- --output-format json -Z unstable-options --document-private-items`

Parsed via `rustdoc-types` 0.57. Post-macro-expansion output.

### Visibility Resolution
- `pub` → always visible
- `pub(crate)` → visible if same crate
- `pub(super)` / `pub(in path)` → visible if observer is in scope
- `default` → hidden (except impl items, delegated to parent type)

### Error Handling
- Missing nightly toolchain: actionable install command
- Package not found: clear message with original cargo error
- Module not found: lists available modules in the crate
- `.with_context()` at each pipeline step

---

## Operational State (v0.1.2)

- Core pipeline complete. All item types supported. 67+ tests (unit + CLI smoke + integration).
- Flexible package name resolution: `self`, `crate::module`, single-arg fallback, file path→module.
- Dependencies: `clap` 4, `rustdoc-types` 0.57, `serde_json` 1, `anyhow` 1.
- Test fixture (`test_fixture/`) covers all supported item types.

---

## Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary backend | rustdoc JSON + `rustdoc-types` | Post-macro-expansion, official, matches LSP-level output |
| `--at-mod` semantics | "compiles when `use`d from here" | Matches developer mental model; includes re-exports |
| Output format | Pseudo-Rust text (not JSON) | LLM consumption; readable as documentation |
| Item-kind filtering | Default=show all common; `--no-*` to exclude; `--all` adds blanket/auto-trait impls | Subtractive model is more ergonomic |
| Statics grouped with constants | `--no-constants` hides both | Conceptually similar; avoids flag proliferation |
| lib.rs + slim main.rs | `run_pipeline()` returns String | Enables integration tests without subprocess |
| External deps | Phase 2 | Adds ~30% complexity; architecture supports it cleanly |
| Target resolution | Semantic layer between CLI and pipeline | `BriefArgs` stays unchanged; resolution in `src/resolve.rs` |
| Single cargo metadata call | `resolve::load_cargo_metadata()` | Eliminates redundant `find_target_dir()` call |
| Ambiguous single arg | Package wins over self-module | Documented; workspace packages checked first |
| File path as module | Heuristic: `/` or `.rs` → file path | 2-level fallback: cwd-relative, then package `src/`-relative |

---

## Active Tickets

- `tickets/wip/260308-visibility-and-rendering.md` — same_crate auto-detection, resolution priority, rendering fixes, external crate JSON investigation
- `tickets/todo/260310-remote-crate-support.md` — `--crates` flag for crates.io crates without local dependency
