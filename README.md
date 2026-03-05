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
cargo brief <crate_name> [module_path] [OPTIONS]
```

### Examples

```sh
# Show the full API of a crate in your workspace
cargo brief my-crate --recursive

# Show a specific module
cargo brief my-crate utils::helpers

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
| `<crate_name>` | Target crate name to inspect |
| `[module_path]` | Module path within the crate (e.g., `my_mod::submod`) |
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

# Full recursive API
cargo brief <crate> --recursive

# Specific module
cargo brief <crate> some::module --recursive

# Multi-workspace: specify manifest path
cargo brief <crate> --manifest-path path/to/Cargo.toml --recursive

# External visibility only (what other crates can see)
cargo brief <crate> --at-package consumer-crate --recursive
```

### Generic LLM Agent

Pipe the output directly into your agent's context:

```sh
# Full crate API
cargo brief some-crate --recursive | your-agent-tool

# Specific module
cargo brief some-crate network::http --recursive | your-agent-tool
```

Or use it as a tool call that returns the output as a string to the agent.

## License

MPL-2.0
