---
title: CLI Surface
summary: All subcommands, flags, positional arguments, and behavioral contracts for the cargo-brief CLI.

features:
  - Invocation Modes
  - Global Flags
    - `-C` / `--crates`
    - `-F` / `--features <FEATURES>`
    - `--no-default-features`
    - `--no-cache`
    - `--toolchain <NAME>`
    - `-v` / `--verbose`
  - Target Resolution
    - Resolution Rules (Local Mode)
    - Key Behaviors
    - Resolution Rules (Remote Mode, `-C`)
    - Smart Leaf Resolution
  - Subcommands
    - `api`
    - `search`
      - Pattern DSL
      - Member Suppression
      - Collapsed Display
      - `--methods-of`
    - `examples`
    - `summary`
    - `ts`
    - `code`
    - `clean`
    - `lsp`
      - `lsp touch`
      - `lsp stop`
      - `lsp status`
      - `lsp references <SYMBOL>`
      - `lsp blast-radius <SYMBOL>`
      - `lsp call-hierarchy <SYMBOL>`
      - Symbol Resolution
  - Shared Option Groups
    - FilterArgs
    - GlobalArgs
    - TargetArgs
  - Behavioral Contracts
    - Visibility Filtering
    - Cross-Crate Accessible Paths
    - Crate-Level Documentation
    - Trait Impl Collapsing
    - Re-Export Expansion
    - Nightly Toolchain Requirement
    - Error Messages
    - Remote Crate Caching
    - Pagination (`--limit`)
---

# CLI Surface

`cargo-brief` is invoked as either `cargo brief <subcommand>` (via Cargo) or
`cargo-brief <subcommand>` (direct). Global flags appear before the subcommand;
subcommand-specific flags appear after.

```
cargo brief [GLOBAL FLAGS] <subcommand> [ARGS...] [OPTIONS...]
```

## Invocation Modes

The binary accepts two invocation forms:

- **Cargo subcommand**: `cargo brief <args>` -- Cargo passes `brief` as the first arg.
  Parsed as `Cargo { Brief(BriefDirect) }`.
- **Direct binary**: `cargo-brief <args>` -- Parsed directly as `BriefDirect`.

Both forms produce identical behavior. Detection is based on whether `args[1] == "brief"`.

A hidden entry point `cargo-brief __lsp-daemon` re-execs into the LSP daemon main loop.
This is not a user-facing subcommand.

## Global Flags

These flags are declared with `global = true` on the top-level `BriefDirect` struct.
They must appear **before** the subcommand name.

### `-C` / `--crates`

Interpret TARGET as a crates.io package spec instead of a local workspace package.
When active, the tool downloads and caches the crate source, then runs the pipeline
against the cached workspace.

Crate spec syntax: `name` (latest), `name@1` (semver-compatible), `name@1.0.200`
(exact pin, 3-component versions get `=` prefix).

### `-F` / `--features <FEATURES>`

Comma-separated list of features to enable. Requires `-C`. Feature-gated items are
invisible in the output without the corresponding features enabled.

### `--no-default-features`

Disable the crate's default features. Requires `-C`. Combine with `-F` to select
an exact feature set.

### `--no-cache`

Skip the persistent cache directory and use a temporary workspace. Requires `-C`.
Useful for forcing a fresh download.

### `--toolchain <NAME>`

Name of the nightly toolchain to use for rustdoc JSON generation. Default: `nightly`.
All subcommands that invoke rustdoc (api, search, summary, and the default dep mode of
code) use this value as `cargo +<toolchain> rustdoc ...`.

### `-v` / `--verbose`

Print progress messages to stderr during pipeline execution. Useful for diagnosing
slow operations (rustdoc generation, cross-crate pre-warming, etc.).

## Target Resolution

Most subcommands accept a TARGET positional argument that identifies which crate (and
optionally which module) to operate on. The resolution algorithm lives in `resolve.rs`
and follows a priority-ordered set of rules.

### Resolution Rules (Local Mode)

| Input                  | Package              | Module          |
|------------------------|----------------------|-----------------|
| `self`                 | Current package (cwd)| None            |
| `self::foo::bar`       | Current package      | `foo::bar`      |
| `self` + `foo::bar`    | Current package      | `foo::bar`      |
| `crate_name::mod`      | `crate_name`         | `mod`           |
| `crate_name` + `mod`   | `crate_name`         | `mod`           |
| `src/foo.rs`           | Current package      | `foo`           |
| `src/foo/bar.rs`       | Current package      | `foo::bar`      |
| `src/foo/mod.rs`       | Current package      | `foo`           |
| `src/lib.rs`           | Current package      | None (root)     |
| `known_workspace_pkg`  | That package         | None            |
| `unknown_name`         | Treated as package   | None            |

### Key Behaviors

- **Bare name always resolves as package.** A single arg without `::` that is not a file
  path is always treated as a package name (workspace-first, then external). Use
  `self::module` for own modules.
- **Hyphen/underscore normalization.** `my_crate` matches workspace package `my-crate`.
- **File path detection.** Strings containing `/` or ending with `.rs` are treated as file
  paths. Fallback search order: cwd-relative, then `<package>/src/`-relative, then
  `<package>/`-relative.
- **`self` requires cwd to be inside a package directory.** Virtual workspace roots
  (with no package) produce an error.

### Resolution Rules (Remote Mode, `-C`)

With `-C`, TARGET is a crates.io spec. The `::module` suffix is supported:
`tokio@1::net` resolves to crate `tokio@1`, module `net`.

### Smart Leaf Resolution

When the final path segment resolves to a leaf item (struct, enum, trait, function, etc.)
rather than a module, the tool resolves the parent module and renders just that item with
full detail (definition + impls). Module names take priority (backward compatible).

## Subcommands

### `api`

Extract and render a crate's API as pseudo-Rust documentation. This is the primary
subcommand.

**Positional arguments:**
- `[TARGET]` -- Package/module to inspect (default: `self`)
- `[MODULE_PATH]` -- Optional module path within the crate

**Subcommand-specific flags:**

| Flag                 | Default | Description                                         |
|----------------------|---------|-----------------------------------------------------|
| `--depth <N>`        | `1`     | How many submodule levels to recurse into            |
| `--recursive`        | off     | Recurse into all submodules (no depth limit)         |
| `--no-expand-glob`   | off     | Show `pub use` lines instead of inlining definitions |
| `--at-package <PKG>` | auto    | Override the observer's package for visibility       |
| `--at-mod <MOD>`     | auto    | Override the observer's module path for visibility   |
| `--manifest-path`    | auto    | Path to Cargo.toml                                   |

Plus shared FilterArgs and GlobalArgs (see below).

**Output format:** Pseudo-Rust text with module headers, doc comments, item definitions.
Function bodies replaced with `;`. Hidden fields shown as `..`. Grouped by module.

### `search`

Search for items by name across a crate. Returns a compact one-line-per-item listing
with kind prefix and full path.

**Positional arguments:**
- `[TARGET]` -- Crate to search (default: `self`)
- `[PATTERN...]` -- Search patterns (0 or more; multiple args are AND-matched)

**Subcommand-specific flags:**

| Flag                    | Default | Description                                      |
|-------------------------|---------|--------------------------------------------------|
| `--limit [OFFSET:]N`   | none    | Limit/paginate results                           |
| `--methods-of <TYPE>`  | none    | Show methods/fields of the named type            |
| `--members`            | off     | Show all members of matched types                |
| `--search-kind <KINDS>`| none    | Filter by item kind (comma-separated)            |
| `--at-package <PKG>`   | auto    | Override observer's package                      |
| `--at-mod <MOD>`       | auto    | Override observer's module path                  |
| `--manifest-path`      | auto    | Path to Cargo.toml                               |

Plus shared FilterArgs and GlobalArgs.

#### Pattern DSL

Patterns follow a mini-DSL with smart-case matching, combinators, and operators.

**Combinators:**
- **Space** between tokens = AND (all must match)
- **Comma** between tokens = OR (any group can match)
- Multiple positional args are joined with spaces (AND semantics)

**Smart-case matching:**
- All-lowercase pattern = case-insensitive
- Any uppercase character = case-sensitive

**Operators (per token):**

| Operator  | Syntax       | Behavior                                           |
|-----------|--------------|----------------------------------------------------|
| Substring | `word`       | Path contains "word"                               |
| Glob      | `w*ld`, `?`  | `*` matches 0+ chars, `?` matches 1 char. Anchored to full path |
| Exact     | `=Name`      | Final `::` segment equals "Name" exactly           |
| Exclude   | `-term`      | Remove matches (global across all OR groups)       |

Exclusion can be combined with other operators: `-=Name` excludes exact matches,
`-*test*` excludes glob matches.

**Note:** Patterns starting with `-` require `--` before them on the command line.

#### Member Suppression

By default, member items (fields, variants, impl methods, associated types/consts) are
suppressed unless a search token exactly matches the member's name. The `--members` flag
expands all members of matched types.

#### Collapsed Display

Consecutive items sharing a parent path render with `-::member` continuation lines
rather than repeating the full path.

#### `--methods-of`

Exact parent-type matching: shows only methods/fields of the named type (not substring
matches). Bypasses member suppression. Zero-result sub-crate headers are suppressed.

### `examples`

Grep example, test, and bench source files from a crate.

**Positional arguments:**
- `[TARGET]` -- Crate to scan (default: `self`)
- `[PATTERN...]` -- Grep patterns (0 or more; multiple args are AND-matched)

**Modes:**
- **List mode** (no pattern): Enumerates `.rs` files with their `//!` doc comments.
- **Grep mode** (pattern given): Shows matching lines with context and `*` markers on
  match lines.

**Subcommand-specific flags:**

| Flag                  | Default | Description                                       |
|-----------------------|---------|---------------------------------------------------|
| `--context <N or B:A>`| `2`     | Lines of context around matches. `N` for symmetric, `B:A` for asymmetric |
| `--tests [DEPTH]`     | off     | Include `tests/` directory. Optional depth (default: unlimited) |
| `--benches [DEPTH]`   | off     | Include `benches/` directory. Optional depth (default: unlimited) |
| `--manifest-path`     | auto    | Path to Cargo.toml                                 |

Plus GlobalArgs.

**Matching:** Smart-case (all-lowercase = insensitive, any uppercase = sensitive).
Multiple pattern arguments are AND-matched.

**Scope:** By default, only the `examples/` directory is scanned. `--tests` and
`--benches` extend the scope to include those directories.

### `summary`

Show a compact module-level overview with item counts per kind.

**Positional arguments:**
- `[TARGET]` -- Crate to summarize (default: `self`)
- `[MODULE_PATH]` -- Optional module path to start from

Plus shared TargetArgs (including `--at-package`, `--at-mod`, `--manifest-path`) and GlobalArgs.

**Output format:** One line per module showing counts of traits, structs, enums,
functions, types, constants, macros, and unions.

### `ts`

Run a tree-sitter structural query against crate source files.

**Positional arguments:**
- `<TARGET>` -- Crate to query (required)
- `<QUERY>` -- Tree-sitter S-expression pattern (required)

**Subcommand-specific flags:**

| Flag                  | Default | Description                                        |
|-----------------------|---------|----------------------------------------------------|
| `--captures`          | off     | Show capture name + text pairs instead of full nodes |
| `--context <N or B:A>`| `0`     | Lines of context around matched nodes               |
| `--src-only`          | off     | Only search `src/` (skip examples, tests, benches)  |
| `--limit [OFFSET:]N`  | none    | Limit/paginate results                              |
| `-q` / `--quiet`      | off     | Output only `@file:line` locations, no source text   |
| `--manifest-path`     | auto    | Path to Cargo.toml                                   |

Plus GlobalArgs.

**Scan scope:** By default scans `src/`, `examples/`, `tests/`, and `benches/`.
`--src-only` restricts to `src/` only.

**Capture behavior:** Queries without captures are auto-augmented with `@_match`
so they still produce output. With `--captures`, each named capture is printed as
`@name: <text>`.

**Query syntax:** Standard tree-sitter S-expression patterns. Predicates supported:
`#eq?`, `#match?`, `#not-eq?`, `#any-of?`.

### `code`

Look up code definitions by kind and name using pre-crafted tree-sitter queries.
Bridges the gap between `search` (API shape, no source) and `ts` (raw S-expressions).

**Positional arguments (1-3):**

The positional argument parsing is context-sensitive:

| Form                        | TARGET   | KIND   | NAME   |
|-----------------------------|----------|--------|--------|
| `code NAME`                 | `self`   | all    | NAME   |
| `code KIND NAME`            | `self`   | KIND   | NAME   |
| `code TARGET NAME`          | TARGET   | all    | NAME   |
| `code TARGET KIND NAME`     | TARGET   | KIND   | NAME   |

**Disambiguation:** When exactly 2 args are given, the first is treated as KIND if it
matches a kind keyword; otherwise it is treated as TARGET. A single arg that is a kind
keyword produces an error (use the 2-arg form).

**Item kinds:** `fn`, `struct`, `enum`, `trait`, `field`, `type`, `impl`, `macro`,
`const`, `use`. Omitting KIND searches all kinds except `use` (to reduce noise).

**Subcommand-specific flags:**

| Flag                  | Default      | Description                                       |
|-----------------------|--------------|---------------------------------------------------|
| `--src-only`          | off          | Only search `src/`                                |
| `--no-deps`           | off          | Target crate only (no dependency search)          |
| `--all-deps`          | off          | All direct deps via cargo metadata (no nightly)   |
| `--limit [OFFSET:]N`  | none         | Limit/paginate results                            |
| `-q` / `--quiet`      | off          | Location + module context only, no source text    |
| `--refs`              | off          | Also show grep-based references after definitions |
| `--refs-only`         | off          | Skip definitions, only show references            |
| `--in <TYPE>`         | none         | Scope to items inside a specific type/impl block  |
| `--manifest-path`     | auto         | Path to Cargo.toml                                |

Plus GlobalArgs.

**Dependency search modes:**
- **Default:** Workspace members + accessible dependencies discovered via rustdoc JSON
  reachability BFS. Requires nightly.
- **`--no-deps`:** Target crate (or all workspace members if TARGET is `self`) only.
- **`--all-deps`:** Workspace members + all direct dependencies resolved via cargo
  metadata. No nightly needed; wider but noisier.

**`self` behavior for `code`:** Unlike other subcommands where `self` = current package,
`code self` searches ALL workspace members. This is because code lookup is a source-level
tool where project-wide search is the common case.

**Name matching:** Smart-case. The name must match the item's identifier (not a
substring).

**Output format:**
```
@<file>:<line>
  in <crate>::<module>[, <parent>]
<source text>
```

With `--quiet`, only the location and module-path lines are shown.

**Reference search (`--refs`, `--refs-only`):** After definitions, grep for literal name
occurrences across the same source files. Match lines marked with `*`, 2 lines of
surrounding context. `--refs-only` skips definitions entirely. `--limit` applies to
definitions (with `--refs`) or grep matches (with `--refs-only`).

**Parent scoping (`--in`):** Filters to items inside a specific type, impl block, or
trait. Uses smart-case matching on the type identifier. Top-level items are excluded.

### `clean`

Clear cached remote crate workspaces.

**Positional arguments:**
- `[SPEC]` -- Crate spec to clean (omit to clean all)

No other flags. Does not use GlobalArgs or FilterArgs.

**Cache location:** `$CARGO_BRIEF_CACHE_DIR`, or `$XDG_CACHE_HOME/cargo-brief/crates`,
or `~/.cache/cargo-brief/crates`.

### `lsp`

Manage a persistent rust-analyzer daemon for cross-reference queries.

The `lsp` subcommand has its own sub-subcommands. The daemon auto-starts on first query
and has a 10-minute idle timeout (override: `CARGO_BRIEF_LSP_TIMEOUT` env var).

**Rejects `-C`** -- LSP commands operate on the local workspace only.

One daemon per workspace root.

**Shared flags:** GlobalArgs, `--manifest-path`.

#### `lsp touch`

Ensure the LSP daemon is running. By default blocks until rust-analyzer finishes
indexing.

| Flag         | Description                              |
|--------------|------------------------------------------|
| `--no-wait`  | Return immediately (fire-and-forget)     |

#### `lsp stop`

Gracefully shut down the daemon.

#### `lsp status`

Show daemon PID, rust-analyzer state, and uptime.

#### `lsp references <SYMBOL>`

Find all references to a symbol via rust-analyzer.

| Flag          | Description                     |
|---------------|---------------------------------|
| `-q` / `--quiet` | Location-only output format |

#### `lsp blast-radius <SYMBOL>`

Show direct and transitive callers of a symbol (BFS).

| Flag             | Default | Description                           |
|------------------|---------|---------------------------------------|
| `--depth <N>`    | `1`     | Depth of transitive caller search (max 10). 1 = direct only |
| `-q` / `--quiet` | off     | Location-only output format           |

#### `lsp call-hierarchy <SYMBOL>`

Show incoming or outgoing call hierarchy for a symbol.

| Flag              | Default | Description                          |
|-------------------|---------|--------------------------------------|
| `--outgoing`      | off     | Show outgoing calls instead of incoming |
| `-q` / `--quiet`  | off     | Location-only output format          |

#### Symbol Resolution

Symbols are resolved in two stages:
1. **Workspace/symbol search** -- fast, finds workspace-defined items.
2. **Fallback:** grep workspace source for usage sites, then resolve via
   `textDocument/definition` -- slower, finds external deps.

Qualified names work: `hecs::World`, `App::new`, `MyStruct::method`.
Common names like `new` may be ambiguous.

## Shared Option Groups

### FilterArgs

Available on `api` and `search` subcommands. Controls which item kinds appear in output
and output density.

**Item kind exclusion (subtractive model -- all shown by default):**

| Flag              | Excludes          |
|-------------------|-------------------|
| `--no-structs`    | Structs           |
| `--no-enums`      | Enums             |
| `--no-traits`     | Traits            |
| `--no-functions`  | Free functions    |
| `--no-aliases`    | Type aliases      |
| `--no-constants`  | Constants AND statics (grouped) |
| `--no-unions`     | Unions            |
| `--no-macros`     | Macros            |

**Output density:**

| Flag                    | Effect                                              |
|-------------------------|-----------------------------------------------------|
| `--no-docs`             | Suppress all doc comments                           |
| `--no-crate-docs`       | Suppress crate-level `//!` documentation only       |
| `--doc-lines <N>`       | Limit doc comments to first N lines (0 = suppress)  |
| `--compact`             | Suppress docs, collapse struct fields/enum variants/trait items |
| `--verbose-metadata`    | Show all attributes (`#[must_use]`, `#[repr(...)]`, etc.) |
| `--all`                 | Show blanket/auto-trait impls (normally collapsed)   |

**Default attribute rendering:** `#[deprecated]` and `#[non_exhaustive]` are always shown.
`--verbose-metadata` adds `#[repr]`, `#[must_use]`, and others.

### GlobalArgs

Available on all subcommands except `clean`. Contains `--toolchain` and `-v`/`--verbose`
(see Global Flags section above for details).

### TargetArgs

Used by `api` and `summary`. Bundles TARGET, MODULE_PATH, `--at-package`, `--at-mod`,
and `--manifest-path`.

## Behavioral Contracts

### Visibility Filtering

All output respects the observer's visibility perspective:
- **External crates:** Only `pub` items are shown.
- **Same crate (auto-detected from cwd):** `pub(crate)` items are included.
- **`--at-mod` override:** `pub(super)`, `pub(in path)` items are included when the
  observer is in scope.

### Cross-Crate Accessible Paths

For facade crates (bevy, axum), items are shown with their user-facing re-export paths
rather than internal module paths. A `CrossCrateIndex` maps items to their shortest
accessible path.

### Crate-Level Documentation

Root module `//!` comments are rendered after the `// crate <name>` header.
`--no-crate-docs` suppresses them independently of `--no-docs`.

### Trait Impl Collapsing

Simple trait impls (no associated items) are collapsed into per-type summary comments
by default. `--all` expands them.

### Re-Export Expansion

By default, glob re-exports (`pub use foo::*`) are expanded inline -- the referenced
items appear as if defined locally. Named re-exports (`pub use foo::Bar`) are also
expanded. `--no-expand-glob` on the `api` subcommand reverts to showing raw `pub use`
lines.

Re-export lines include kind annotations as comments: `pub use foo::Bar; // struct`.

### Nightly Toolchain Requirement

Subcommands that generate rustdoc JSON (`api`, `search`, `summary`, and `code` in
default dep mode) require a nightly Rust toolchain. The tool runs a pre-check via
`rustup which` and, when on a TTY, offers an interactive install prompt if the toolchain
is missing. Non-TTY contexts get an actionable error message.

### Error Messages

- **Package not found:** Shows the original cargo error.
- **Module not found:** Lists available modules in the crate.
- **Leaf item not found:** Lists available items in the parent module (visibility-filtered).
- **`self` in workspace root:** Explains that virtual workspace roots have no package.

### Remote Crate Caching

Cached workspaces are stored at `~/.cache/cargo-brief/crates/` with version-normalized
directory names (`name[version]` or `name[version]+feat1+feat2`). Bare specs (no
`@version`) auto-update. Exact versions are resolved via the crates.io API with 24-hour
cache. `cargo brief clean [SPEC]` manages disk usage.

### Pagination (`--limit`)

Available on `search`, `ts`, and `code`. Syntax: `N` (first N results) or `OFFSET:N`
(skip OFFSET, then show N). Applies to definitions for `code --refs`, or to grep matches
for `code --refs-only`.
