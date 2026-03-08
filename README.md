# cargo-brief

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
cargo brief <target> [module_path] [OPTIONS]
```

The first argument is flexible — it can be a crate name, `self`, a `crate::module` path, or even a file path:

| Syntax | Resolves to |
|--------|-------------|
| `my-crate` | Named crate (hyphen/underscore normalized) |
| `self` | Current package (detected from cwd) |
| `self::module` | Current package, specific module |
| `crate::module` | Named crate + module in one arg |
| `src/foo.rs` | File path → auto-converted to module path |
| `unknown_name` | If not a workspace package → treated as self module |

### Examples

```sh
# Show the full API of a crate in your workspace
cargo brief my-crate --recursive

# Inspect the current package
cargo brief self --recursive

# Show a specific module (multiple syntaxes)
cargo brief my-crate utils::helpers
cargo brief self::utils
cargo brief src/utils.rs

# Show only what's visible from an external crate
cargo brief my-crate --at-package other-crate

# Limit recursion depth
cargo brief my-crate --depth 2

# Exclude certain item kinds
cargo brief my-crate --no-macros --no-traits
```

## Options

| Flag | Description |
|------|-------------|
| `<target>` | Target to inspect: crate name, `self`, `crate::module`, or file path |
| `[module_path]` | Module path within the crate (e.g., `my_mod::submod` or `src/my_mod.rs`) |
| `--at-package <pkg>` | Caller's package name (for visibility resolution) |
| `--at-mod <path>` | Caller's module path (determines what is visible) |
| `--depth <n>` | How many submodule levels to recurse into (default: 1) |
| `--recursive` | Recurse into all submodules (no depth limit) |
| `--all` | Show all item kinds including blanket/auto-trait impls |
| `--no-structs` | Exclude structs |
| `--no-enums` | Exclude enums |
| `--no-traits` | Exclude traits |
| `--no-functions` | Exclude free functions |
| `--no-aliases` | Exclude type aliases |
| `--no-constants` | Exclude constants and statics |
| `--no-unions` | Exclude unions |
| `--no-macros` | Exclude macros |
| `--toolchain <name>` | Nightly toolchain name (default: `nightly`) |
| `--manifest-path <path>` | Path to Cargo.toml |

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

## AI Agent Setup

### Claude Code

Add a note to your project's `CLAUDE.md` so the AI knows to use cargo-brief when exploring crate APIs:

```markdown
## Exploring Crate APIs

Use `cargo brief` to inspect crate interfaces instead of reading source files directly:

# Current package API
cargo brief self --recursive

# Specific module (by name or file path)
cargo brief self::some_module --recursive
cargo brief src/some_module.rs --recursive

# Named crate in workspace
cargo brief <crate> --recursive

# Multi-workspace: specify manifest path
cargo brief <crate> --manifest-path path/to/Cargo.toml --recursive

# External visibility only (what other crates can see)
cargo brief <crate> --at-package consumer-crate --recursive
```

### Generic LLM Agent

Pipe the output directly into your agent's context:

```sh
# Current package API
cargo brief self --recursive | your-agent-tool

# Specific module
cargo brief self::network::http --recursive | your-agent-tool
```

Or use it as a tool call that returns the output as a string to the agent.

## License

MPL-2.0
