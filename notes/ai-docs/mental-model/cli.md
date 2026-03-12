# cli — CLI Argument Definitions

**File:** `src/cli.rs`

## Public Types

### `Cargo`
```rust
#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo", version)]
pub struct Cargo {
    pub command: CargoCommand,  // #[command(subcommand)]
}
```

### `CargoCommand`
```rust
#[derive(Subcommand)]
pub enum CargoCommand {
    Brief(BriefArgs),
}
```

### `BriefArgs`
Core configuration struct, used throughout the pipeline.

```rust
#[derive(Parser, Clone)]
pub struct BriefArgs {
    // Positional
    pub crate_name: String,           // TARGET: crate name, "self", crate::module, or file path
    pub module_path: Option<String>,  // MODULE_PATH: optional module or file path

    // Visibility
    pub at_package: Option<String>,   // --at-package: caller's package
    pub at_mod: Option<String>,       // --at-mod: caller's module path

    // Recursion
    pub depth: u32,                   // --depth (default: 1)
    pub recursive: bool,             // --recursive (no depth limit)

    // Item filtering
    pub all: bool,                   // --all: include blanket/auto-trait impls
    pub no_structs: bool,
    pub no_enums: bool,
    pub no_traits: bool,
    pub no_functions: bool,
    pub no_aliases: bool,
    pub no_constants: bool,          // also hides statics
    pub no_unions: bool,
    pub no_macros: bool,

    // Build
    pub toolchain: String,           // --toolchain (default: "nightly")
    pub manifest_path: Option<String>,
}
```

## Design Notes

- Subtractive filtering: all common items shown by default, `--no-*` to exclude.
- `--all` adds blanket/auto-trait impls (normally hidden).
- Statics grouped under `--no-constants`.
- `BriefArgs` is `Clone` for use in tests and across pipeline stages.

## Dual Invocation (`main.rs`)

`main.rs::parse_args()` handles two modes:
1. **`cargo brief ...`** — parses `Cargo` struct, extracts `BriefArgs` from `CargoCommand::Brief`
2. **`cargo-brief ...`** — parses `BriefArgs` directly

Detection: checks if `args[1] == "brief"`.
