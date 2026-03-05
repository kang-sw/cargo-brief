use clap::{Parser, Subcommand};

/// Cargo subcommand wrapper.
#[derive(Parser, Debug)]
#[command(name = "cargo", bin_name = "cargo")]
pub struct Cargo {
    #[command(subcommand)]
    pub command: CargoCommand,
}

#[derive(Subcommand, Debug)]
pub enum CargoCommand {
    /// Extract and display Rust crate API as pseudo-Rust documentation.
    Brief(BriefArgs),
}

/// Core arguments for cargo-brief.
#[derive(Parser, Debug, Clone)]
pub struct BriefArgs {
    /// Target crate name to inspect
    pub crate_name: String,

    /// Module path within the crate to inspect (e.g., "my_mod::submod")
    pub module_path: Option<String>,

    /// Caller's package name (for visibility resolution)
    #[arg(long)]
    pub at_package: Option<String>,

    /// Caller's module path (determines what is visible)
    #[arg(long)]
    pub at_mod: Option<String>,

    /// How many submodule levels to recurse into
    #[arg(long, default_value = "1")]
    pub depth: u32,

    /// Recurse into all submodules (no depth limit)
    #[arg(long)]
    pub recursive: bool,

    /// Show all item kinds including blanket/auto-trait impls
    #[arg(long)]
    pub all: bool,

    // === Exclusion flags (default: all common items shown) ===
    /// Exclude structs
    #[arg(long)]
    pub no_structs: bool,

    /// Exclude enums
    #[arg(long)]
    pub no_enums: bool,

    /// Exclude traits
    #[arg(long)]
    pub no_traits: bool,

    /// Exclude free functions
    #[arg(long)]
    pub no_functions: bool,

    /// Exclude type aliases
    #[arg(long)]
    pub no_aliases: bool,

    /// Exclude constants and statics
    #[arg(long)]
    pub no_constants: bool,

    /// Exclude macros
    #[arg(long)]
    pub no_macros: bool,

    /// Nightly toolchain name (default: "nightly")
    #[arg(long, default_value = "nightly")]
    pub toolchain: String,

    /// Manifest path (passed to cargo)
    #[arg(long)]
    pub manifest_path: Option<String>,
}
