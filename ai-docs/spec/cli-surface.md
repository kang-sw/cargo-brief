---
title: CLI Surface
summary: All subcommands, flags, positional arguments, target resolution rules, and behavioral contracts for the cargo-brief CLI.
features:
  - Invocation Modes
  - Global Flags
    - -C / --crates
    - -F / --features \<FEATURES\>
    - --no-default-features
    - --no-cache
    - --toolchain \<NAME\>
    - -v / --verbose
  - Target Resolution
    - Smart Leaf Resolution
  - api Subcommand
  - search Subcommand
    - Pattern DSL
    - Member Display
  - examples Subcommand
  - summary Subcommand
  - ts Subcommand
  - code Subcommand
    - Dependency Search Modes
  - features Subcommand
  - clean Subcommand
  - lsp Subcommand — Lifecycle Commands
  - lsp Subcommand — Query Commands
  - Shared Option Groups
    - FilterArgs
    - GlobalArgs
    - TargetArgs
  - Behavioral Contracts
    - Nightly Toolchain Detection
    - AI Agent Quick Guide
    - Error Messages
---

# CLI Surface

`cargo-brief` is invoked as either a Cargo subcommand (`cargo brief`) or a standalone binary (`cargo-brief`). All eight subcommands share a common set of global flags and shared option groups; each subcommand defines its own positional arguments and subcommand-specific flags.

## Invocation Modes {#260423-invocation-modes}

Two invocation forms produce identical behavior:

- `cargo brief <subcommand> [args…]` — Cargo plugin form. Cargo passes all arguments after `brief` to the binary.
- `cargo-brief <subcommand> [args…]` — Direct binary form.

The binary detects the form by inspecting whether the first argument equals `"brief"`.

A hidden re-exec entry point `cargo-brief __lsp-daemon` starts the LSP daemon as a detached child process. It is not surfaced in `--help` and is not intended for direct user invocation, but it must remain stable because the client process re-execs the same binary with this argument.

## Global Flags

Global flags appear on `BriefDirect` with `global = true`. They must be placed **before** the subcommand name on the command line.

### -C / --crates {#260423-global-flag-crates}

Switches to remote crates.io mode. When set, `TARGET` is interpreted as a crate spec rather than a local package name. Crate spec syntax:

| Form | Meaning |
|---|---|
| `name` | Latest published version |
| `name@1` | Highest semver-compatible with `^1` |
| `name@1.2` | Highest semver-compatible with `^1.2` |
| `name@1.0.200` | Exact pin (`=1.0.200`) |

The `::module` suffix is supported in remote mode (e.g. `tokio@1::net`).

### -F / --features \<FEATURES\> {#260423-global-flag-features}

Comma-separated list of Cargo features to enable. Requires `-C`.

Feature names are validated against the crate's feature graph before invoking `cargo rustdoc`. {#260423-feature-flag-validation} If an unknown name is passed, cargo-brief exits with an error and suggests the closest valid name using Jaro-Winkler similarity:

```
error: unknown feature "asynch"
  --> did you mean: "async"?
```

If no feature graph is available (network failure in remote mode), validation is skipped and the raw `-F` list is forwarded to cargo unchanged.

### --no-default-features {#260423-global-flag-no-default-features}

Disables the crate's default feature set. Requires `-C`.

### --no-cache {#260423-global-flag-no-cache}

Forces a fresh workspace rather than reusing the persistent cache. Requires `-C`.

### --toolchain \<NAME\> {#260423-global-flag-toolchain}

Nightly toolchain name passed to `rustup` and `cargo +<toolchain>`. Default: `nightly`.

Consumed by `api`, `search`, `summary`, and `code` (default dep mode). Subcommands that do not invoke `cargo rustdoc` (`ts`, `examples`, `code --no-deps`, `lsp`, `clean`) accept the flag but ignore it.

### -v / --verbose {#260423-global-flag-verbose}

Prints pipeline progress to stderr at key stages: target resolution, `cargo rustdoc` invocation, cache hits, and cross-crate discovery.

## Target Resolution {#260423-target-resolution}

Target resolution maps the `TARGET` positional argument (and optional `MODULE_PATH`) to a `(package, optional_module)` pair. The algorithm runs in `src/resolve.rs`.

| Input form | Package | Module |
|---|---|---|
| _(omitted)_ | current package | none |
| `self` | current package | none |
| `self::foo::bar` | current package | `foo::bar` |
| `crate::foo::bar` | `crate` package | `foo::bar` |
| `name` (bare) | `name` package | none |
| `name::foo` | `name` package | `foo` |
| `src/foo.rs` | current package | `foo` |
| `src/foo/mod.rs` | current package | `foo` |
| `src/lib.rs` | current package | none (crate root) |
| `name foo::bar` (two args) | `name` | `foo::bar` |

Key behaviors:

- A bare single argument always resolves as a **package name**, never as a module path.
- Hyphen and underscore are normalized interchangeably when looking up workspace packages.
- File path detection: a string is treated as a file path if it contains `/` or ends with `.rs`. The tool searches cwd-relative, then `src/<file>`, then `<pkg>/<file>`.
- `self` in a virtual workspace root (no package in cwd) produces: `"Cannot resolve 'self': no package found for the current directory. Are you in a package directory? (Virtual workspace roots have no package.)"`
- A `self::` prefix in the `MODULE_PATH` second argument is stripped automatically.

### Smart Leaf Resolution {#260423-smart-leaf-resolution}

When the final segment of a resolved path does not name a module, the tool interprets it as a leaf item (struct, enum, trait, fn, type alias, etc.). It renders the parent module filtered to show only that item with full impl and method detail.

Module names take priority over leaf names when both exist. `pub use` chains are followed across crate boundaries.

## api Subcommand {#260423-api-subcommand}

Renders a crate or module's API as pseudo-Rust documentation.

```
cargo brief api [TARGET] [MODULE_PATH] [OPTIONS]
```

Positionals: `[TARGET]` (default `self`), `[MODULE_PATH]` (optional).

Subcommand-specific flags:

| Flag | Default | Description |
|---|---|---|
| `--depth <N>` | `1` | Submodule recursion depth |
| `--recursive` | off | Recurse into all submodules (unlimited depth) |
| `--no-expand-glob` | off | Show raw `pub use` lines instead of inlining glob re-exports |

Also accepts FilterArgs and GlobalArgs. TargetArgs (`--at-package`, `--at-mod`, `--manifest-path`) are bundled via the shared group.

## search Subcommand {#260423-search-subcommand}

Searches all visible items by name. Outputs one line per item.

```
cargo brief search [TARGET] [PATTERN…] [OPTIONS]
```

Positionals: `[TARGET]` (default `self`), `[PATTERN…]` (zero or more, joined with spaces).

Subcommand-specific flags:

| Flag | Description |
|---|---|
| `--limit [OFFSET:]N` | Paginate results |
| `--methods-of <TYPE>` | Show only items whose direct parent is `TYPE` (exact match) |
| `--search-kind <KINDS>` | Comma-separated kind filter: `fn`, `struct`, `enum`, `trait`, `field`, `variant`, `const`, `static`, `type`, `macro`, `use` |
| `--members` | Expand all members (fields, methods, trait impls) of matched types |
| `--at-package`, `--at-mod` | Visibility observer override |
| `--manifest-path` | Path to `Cargo.toml` |

Also accepts FilterArgs and GlobalArgs.

### Pattern DSL {#260423-search-pattern-dsl}

Multiple tokens on the command line join with spaces (no quoting needed for multi-word AND). Comma separates OR groups.

| Operator | Example | Meaning |
|---|---|---|
| Substring (default) | `reader` | Any item whose path contains `reader` |
| Glob | `Camera*` | Glob match against the **full** item path |
| Exact | `=Router` | Final `::` segment equals `Router` exactly |
| Exclude | `-test` | Exclude items matching `test`; applies across all OR groups |
| Combined | `-=Internal`, `-*test*` | Combine exclude with other operators |

Smart-case applies to all operators: an all-lowercase pattern is case-insensitive; any uppercase letter makes the match case-sensitive.

Patterns beginning with `-` require `--` on the command line to prevent flag parsing.

> [!note] Implementation Gap · 2026-04-23
> Glob patterns are matched against the full item path, so `Camera*` returns zero results unless the crate name itself starts with `Camera`. The intended behavior is to match against the final `::` segment by default. Current workaround: use a substring pattern (`camera`) instead.

### Member Display {#260423-search-member-display}

By default, member items (fields, methods, variants) are suppressed in search results unless a pattern token exactly matches the member name.

`--members` expands all members of every matched type, showing fields, inherent methods, and trait impl methods.

`--methods-of <TYPE>` bypasses member suppression for the named type and shows only its items. `TYPE` is an exact parent-type match, not a substring.

Consecutive items sharing a parent path use a `-::member` continuation line to avoid repeating the full path.

Zero-result sub-crate headers are suppressed in multi-crate searches unless `--verbose` is set.

## examples Subcommand {#260423-examples-subcommand}

Lists or greps `examples/`, `tests/`, and `benches/` source files.

```
cargo brief examples [TARGET] [PATTERN…] [OPTIONS]
```

Positionals: `[TARGET]` (default `self`), `[PATTERN…]` (zero or more).

Two modes:
- **List mode** (no pattern): enumerates `.rs` files with their `//!` module-level doc comment (first line).
- **Grep mode** (pattern present): finds matching lines with context. Match lines are prefixed with `*`; context lines with a space; non-adjacent groups separated by `…`.

Flags:

| Flag | Default | Description |
|---|---|---|
| `--context <N or B:A>` | `2` | Context lines before and after each match (`B:A` for asymmetric) |
| `--tests [DEPTH]` | off | Include `tests/` up to DEPTH levels deep (omit DEPTH = unlimited) |
| `--benches [DEPTH]` | off | Include `benches/` up to DEPTH levels deep (omit DEPTH = unlimited) |
| `--manifest-path` | auto | Path to `Cargo.toml` |

Default scope: `examples/` only. Smart-case matching applies.

When no examples exist, an informative message is printed — not an error exit.

## summary Subcommand {#260423-summary-subcommand}

Prints a compact module-level table of contents showing item counts per kind.

```
cargo brief summary [TARGET] [MODULE_PATH] [OPTIONS]
```

Output: one line per visible submodule, annotated with counts: `mod io; // 4 traits, 15 structs, 8 fns`. Zero-count kinds are omitted. The visibility system and reachable set are respected.

Uses TargetArgs and GlobalArgs only. No FilterArgs.

## ts Subcommand {#260423-ts-subcommand}

Runs tree-sitter S-expression structural queries against crate source files.

```
cargo brief ts <TARGET> '<QUERY>' [OPTIONS]
```

Both positionals are required; `TARGET` has no default.

Output modes (mutually exclusive):

| Mode | Flag | Output |
|---|---|---|
| Verbatim (default) | — | Matched node source with `@file:line` header |
| Captures | `--captures` | `@name: <text>` pairs for each named capture |
| Context | `--context <N or B:A>` | Matched node with surrounding source lines (default 0) |
| Quiet | `-q` / `--quiet` | Location only: `@file:line` |

Other flags:

| Flag | Description |
|---|---|
| `--src-only` | Restrict scan to `src/` (skip `examples/`, `tests/`, `benches/`) |
| `--limit [OFFSET:]N` | Paginate results |

Default scan scope: `src/`, `examples/`, `tests/`, `benches/`.

Queries without explicit captures are auto-augmented with `@_match` to return the full matched node.

Supported predicates: `#eq?`, `#match?`, `#not-eq?`, `#any-of?`.

Works with `-C` for remote crates.

## code Subcommand {#260423-code-subcommand}

Looks up code definitions by item kind and name using pre-crafted tree-sitter queries.

```
cargo brief code [TARGET] [KIND] <NAME> [OPTIONS]
```

Accepts 1–3 positional arguments with context-sensitive disambiguation:

| Arg count | Interpretation |
|---|---|
| 1 | `NAME` — search all workspace members, all kinds |
| 2 | If first arg is a kind keyword: `KIND NAME` with `self` target. Otherwise: `TARGET NAME`, all kinds. |
| 3 | `TARGET KIND NAME` |

A single argument that is a valid kind keyword alone is an error — use the 2-arg or 3-arg form.

Supported kinds: `fn`, `struct`, `enum`, `trait`, `field`, `type`, `impl`, `macro`, `const`, `use`. Omitting KIND also excludes `use` from results.

Output per match: `@<file>:<line>`, `in <crate>::<module>[, in <parent>]`, then the matched source block.

Flags:

| Flag | Description |
|---|---|
| `--refs` | Append grep-based reference sites after each definition |
| `--refs-only` | Show only references, skip definitions (conflicts with `--refs`) |
| `--in <TYPE>` | Scope results to items inside a specific type, impl block, or trait |
| `--src-only` | Skip non-`src/` files |
| `--limit [OFFSET:]N` | Paginate. For `--refs`, applies to definitions; for `--refs-only`, to grep matches. |
| `-q` / `--quiet` | Location only |
| `--manifest-path` | Path to `Cargo.toml` |

Name matching is smart-case and must match a complete identifier, not a substring.

`self` as `TARGET` searches **all workspace members** simultaneously (unlike other subcommands where `self` means the current package only).

### Dependency Search Modes {#260423-code-dep-search-modes}

| Mode | Flag | Scope | Nightly required |
|---|---|---|---|
| Default | — | Workspace members + BFS-reachable deps via rustdoc JSON | Yes |
| No deps | `--no-deps` | Target crate only (or all workspace members when `TARGET=self`) | No |
| All deps | `--all-deps` | Workspace members + all direct deps via cargo metadata | No |

Works with `-C` for remote crates.

## features Subcommand {#260423-features-subcommand}

Shows the Cargo feature graph for a crate as pseudo-TOML.

```
cargo brief features [CRATE] [OPTIONS]
cargo brief -C features <CRATE_SPEC> [OPTIONS]
```

Positional: `[CRATE]` — a local package name or `self` (default). In remote mode (`-C`), the positional is a crate spec.

Flags: `GlobalArgs`, `--manifest-path`. No FilterArgs.

Output format: a `[features]` TOML block. `default = [...]` appears first; all other features follow alphabetically. Features that correspond to optional dependencies are annotated with `# optional dep`:

```toml
[features]
default = ["std"]
derive = []
serde1 = ["dep:serde"]  # optional dep
std = ["alloc"]
```

In remote mode (`-C`), the feature graph is fetched from the crates.io API (see [Remote Feature Graph](#260423-remote-feature-graph) in the Remote Crates spec). If the network is unavailable, the subcommand exits with an error.

In local mode, the feature graph is extracted from `cargo metadata` output. `self` resolves to the current package; a package name selects a specific workspace member.

## clean Subcommand {#260423-clean-subcommand}

Deletes cached remote crate workspaces.

```
cargo brief clean [SPEC]
```

`SPEC` is optional. Omitting it cleans all cached workspaces. Providing a spec prefix cleans matching entries only.

Does not accept FilterArgs or GlobalArgs.

## lsp Subcommand — Lifecycle Commands {#260423-lsp-subcommand-lifecycle}

Manages a persistent rust-analyzer daemon per Cargo workspace. The daemon is keyed by the workspace root path. Idle timeout defaults to 10 minutes (`CARGO_BRIEF_LSP_TIMEOUT` env var, in seconds).

The `lsp` subcommand rejects `-C`.

```
cargo brief lsp touch [--no-wait]
cargo brief lsp stop
cargo brief lsp status
```

- **`touch`** — Starts the daemon if not running; blocks until rust-analyzer finishes indexing by default. `--no-wait` returns immediately after spawn.
- **`stop`** — Graceful daemon shutdown.
- **`status`** — Shows daemon PID, rust-analyzer indexing state, and uptime.

All `lsp` sub-subcommands accept `--manifest-path` and `GlobalArgs`.

## lsp Subcommand — Query Commands {#260423-lsp-subcommand-queries}

Semantic analysis commands that require a running daemon (auto-started if absent).

```
cargo brief lsp references <SYMBOL> [-q]
cargo brief lsp blast-radius <SYMBOL> [--depth <N>] [-q]
cargo brief lsp call-hierarchy <SYMBOL> [--outgoing] [-q]
```

- **`references`** — All reference sites for `SYMBOL`, grouped by file with surrounding source context. `-q` outputs locations only.
- **`blast-radius`** — Transitive incoming callers via BFS. `--depth N` controls traversal depth (default 1, max 10). `-q` for locations only.
- **`call-hierarchy`** — Direct incoming or outgoing call tree. `--outgoing` flips to callers of `SYMBOL`'s callees. `-q` for locations only.

Symbol resolution is two-stage: `workspace/symbol` LSP request first; if that returns nothing, a grep-based fallback scans `.rs` files and resolves the definition via `textDocument/definition`. Qualified names are supported (`hecs::World`, `App::new`).

## Shared Option Groups

### FilterArgs {#260423-filter-args}

Available on `api` and `search`. Two categories:

**Item-kind exclusion (subtractive — omit to include):**

`--no-structs`, `--no-enums`, `--no-traits`, `--no-functions`, `--no-aliases`, `--no-constants` (also hides statics), `--no-unions`, `--no-macros`

**Output density:**

| Flag | Effect |
|---|---|
| `--no-docs` | Strip all doc comment lines |
| `--no-crate-docs` | Strip only the crate-level `//!` block |
| `--doc-lines <N>` | Limit each doc comment to first N lines; `0` equals `--no-docs` |
| `--compact` | Collapse struct fields, enum variants, and trait items to `{ .. }`; implies `--no-docs` |
| `--verbose-metadata` | Render `#[repr(…)]`, `#[must_use]`, `#[no_mangle]`, etc. |
| `--all` | Include blanket impls and auto-trait impls; disable trait impl collapsing |
| `--no-feature-gates` | Suppress `#[cfg(feature = "...")]` annotations on items (see [Feature-Gate Annotations](output-format.md#260423-feature-gate-annotations)) |

Default attribute rendering (always on): `#[deprecated]`, `#[non_exhaustive]`.

### GlobalArgs {#260423-global-args}

Available on all subcommands except `clean`. Contains `--toolchain` and `-v` / `--verbose` (described under [Global Flags](#260423-global-flag-toolchain)).

### TargetArgs {#260423-target-args}

Used by `api` and `summary`. Bundles `TARGET`, `MODULE_PATH`, `--at-package`, `--at-mod`, and `--manifest-path`.

`--at-package <PKG>` and `--at-mod <PATH>` override the visibility observer position. See the [Visibility Semantics](visibility.md) spec for full semantics.

## Behavioral Contracts

### Nightly Toolchain Detection {#260423-nightly-toolchain-detection}

Before the first `cargo +nightly rustdoc` invocation in a process, the tool checks toolchain availability via `rustup which rustdoc --toolchain <toolchain>`. This check runs at most once per process.

| Condition | Behavior |
|---|---|
| `rustup` not installed | Error: `"rustup is not installed. Install it from https://rustup.rs/"` |
| Toolchain missing, TTY available | Interactive prompt `"Install it now? [y/N]"`, reads from `/dev/tty` (Unix) or `CONIN$` (Windows). Runs `rustup toolchain install` on `y`/`Y`. |
| Toolchain missing, non-TTY | Error with install command: `"Install it with: rustup toolchain install <toolchain>"` |
| Toolchain present | Silent; proceeds to invocation |

### AI Agent Quick Guide {#260423-ai-agent-quick-guide}

`cargo brief --help` (long form) appends an AI agent quick guide after the standard flag list. The guide maps common situations to subcommands and describes a recommended workflow. `-h` (short form) omits the guide and shows concise flag help only.

### Error Messages {#260423-error-messages}

| Situation | Message |
|---|---|
| Module not found | Lists available modules at the resolved package; appends `--search` tip for remote/facade crates |
| Leaf item not found | Lists available items in the parent module, filtered by visibility |
| Package not found | Echoes the original cargo error; appends `--features` tip |
| File path not found | Lists all three searched locations: cwd-relative, `src/`-relative, pkg-relative |
| `self` in virtual workspace root | Explains the virtual workspace root limitation |
| Ambiguous crate version | Auto-selects highest semver candidate; falls back to `"Use name@version to disambiguate"` |
