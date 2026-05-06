# cargo-brief

[![Crates.io](https://img.shields.io/crates/v/cargo-brief.svg)](https://crates.io/crates/cargo-brief)
[![Downloads](https://img.shields.io/crates/d/cargo-brief.svg)](https://crates.io/crates/cargo-brief)
[![Docs.rs](https://docs.rs/cargo-brief/badge.svg)](https://docs.rs/cargo-brief)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL--2.0-blue.svg)](https://opensource.org/licenses/MPL-2.0)

Visibility-aware Rust API extractor — pseudo-Rust output for AI agent consumption.

## Why?

AI coding agents need to understand crate APIs without reading full source files. HTML docs waste tokens on navigation chrome, and `cargo doc --json` is too verbose. `cargo-brief` outputs concise pseudo-Rust that fits in a context window:

- Function bodies replaced with `;`
- Only items visible from the caller's perspective
- Doc comments preserved verbatim
- Compact module hierarchy

## Installation

```sh
cargo install cargo-brief
```

**Requires the nightly toolchain** (for `rustdoc --output-format json`):

```sh
rustup toolchain install nightly
```

## Usage

```sh
cargo brief [GLOBAL FLAGS] <subcommand> [ARGS...] [OPTIONS...]
```

Global flags (`-C`, `-F`, `--no-cache`, …) must appear **before** the subcommand name.

### Subcommands

| Subcommand | Purpose |
|---|---|
| `api` | Extract API as pseudo-Rust documentation |
| `search` | Search for items by name |
| `summary` | Compact module overview with item counts |
| `examples` | Grep example/test/bench source files |
| `ts` | Run a tree-sitter structural query against source |
| `code` | Look up definitions by kind and name |
| `lsp` | Manage persistent rust-analyzer (references, call graph) |
| `clean` | Clear cached remote crate workspaces |

Run `cargo brief <subcommand> --help` for full options.

---

## Core Subcommands

### `api` — render a crate's API

```sh
# Current package (run inside a Cargo project)
cargo brief api

# Specific module
cargo brief api self::net::tcp

# Recurse all submodules
cargo brief api self --recursive

# Limit depth
cargo brief api self --depth 2

# Exclude item kinds
cargo brief api self --no-macros --no-traits

# Compact output (no doc comments, collapsed bodies)
cargo brief api self --compact

# Show only what's visible from an external caller
cargo brief api my-crate --at-package consumer-crate
```

**Target resolution** — the `[TARGET]` argument accepts:

| Syntax | Resolves to |
|---|---|
| `self` | Current package (cwd-based detection) |
| `self::module` | Current package, specific module |
| `crate::module` | Named crate + module in one arg |
| `src/foo.rs` | File path → auto-converted to module path |
| `my-crate` | Named workspace package |
| `unknown_name` | Treated as package name |

### `search` — find items by name

```sh
# Substring search (smart-case: all-lowercase = insensitive)
cargo brief search self spawn

# Multiple patterns are AND-matched
cargo brief search self "spawn async"

# Show methods/fields of a type
cargo brief search self --methods-of Handle

# Find functions by signature shape
cargo brief search self "" --in-params PathBuf
cargo brief search self parse --in-returns "Result -Vec"

# OR-match with comma
cargo brief search self "EventReader,EventWriter"

# Glob and exact-match operators
cargo brief search self "Shader*Ref"
cargo brief search self "=Router"
```

**Pattern DSL:** space = AND, comma = OR. Operators per token:
- `word` — substring match
- `w*ld` — glob (`*` = 0+ chars, `?` = 1 char)
- `=Name` — exact final segment
- `-term` — exclude (use `--` for patterns starting with `-`)

`--in-params` and `--in-returns` reuse the same pattern syntax against rendered
type strings. Quote multi-token type patterns such as `"TokenStream -Option"`.

### `summary` — module overview

```sh
cargo brief summary self
cargo brief summary self::net
```

One line per module showing counts of traits, structs, enums, functions, types, constants, macros, and unions.

---

## Remote Crates (`-C`)

Pass `-C` **before the subcommand** to treat TARGET as a crates.io spec:

```sh
# Overview of a crates.io package
cargo brief -C summary tokio@1

# Browse a module
cargo brief -C api tokio@1 net

# Search
cargo brief -C search serde@1 Serialize

# Feature-gated APIs (omit -F and items are invisible)
cargo brief -C -F rt,net api tokio@1 --compact
```

Crate spec syntax: `name` (latest), `name@1` (semver-compatible), `name@1.2.3` (exact pin).
Downloaded crates are cached at `~/.cache/cargo-brief/crates/`. Run `cargo brief clean` to clear.

---

## Output Format

```rust
// crate my_crate
mod utils {
    /// Computes the hash of the input.
    pub fn hash(input: &[u8]) -> u64;

    pub struct Config {
        pub timeout: Duration,
        pub retries: u32,
        // ... private fields
    }

    impl Config {
        pub fn new() -> Self;
        pub fn with_timeout(self, timeout: Duration) -> Self;
    }

    pub trait Processor: Send + Sync {
        type Output;
        fn process(&self, input: &[u8]) -> Self::Output;
    }
}
```

---

## AI Agent Setup

### Claude Code

Add to your project's `CLAUDE.md`:

```markdown
## Exploring Crate APIs

Use `cargo brief` to inspect crate interfaces instead of reading source files directly.

# Typical workflow
cargo brief summary self                     # overview of modules
cargo brief api self::some_module            # drill into a module
cargo brief search self SomeType --members   # find a specific item
cargo brief code fn some_function --refs     # read source + references

# Named workspace crate
cargo brief api my-crate --recursive

# External visibility only
cargo brief api my-crate --at-package consumer-crate

# Remote crate from crates.io
cargo brief -C summary tokio@1
cargo brief -C search serde@1 Deserialize

# Specify manifest when outside the workspace root
cargo brief api self --manifest-path path/to/Cargo.toml
```

### Generic LLM Agent

```sh
cargo brief api self --recursive | your-agent-tool
cargo brief api self::network::http | your-agent-tool
```

---

## License

MPL-2.0
