use clap::{Args, Parser, Subcommand};

/// Cargo subcommand wrapper.
#[derive(Parser, Debug)]
#[command(name = "cargo", bin_name = "cargo", version)]
pub struct Cargo {
    #[command(subcommand)]
    pub command: CargoCommand,
}

#[derive(Subcommand, Debug)]
pub enum CargoCommand {
    /// Extract and display Rust crate API as pseudo-Rust documentation.
    Brief(BriefDirect),
}

/// Direct invocation wrapper (`cargo-brief <subcommand> ...`).
#[derive(Parser, Debug)]
#[command(
    version,
    about = "Visibility-aware Rust API extractor for AI agents",
    after_help = "Run `cargo brief <subcommand> --help` for subcommand-specific options."
)]
pub struct BriefDirect {
    #[command(subcommand)]
    pub command: BriefCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum BriefCommand {
    /// Extract and render crate API as pseudo-Rust documentation
    #[command(after_help = "\
EXAMPLES:
  # Browse the current crate's API (run inside a Cargo project)
  cargo brief api

  # Browse a specific module in the current crate
  cargo brief api self::net::tcp

  # Inspect a crates.io dependency (cached after first run)
  cargo brief api --crates serde@1 --compact
  cargo brief api --crates tokio@1 --features rt,net,io-util

  # Browse a specific module of a remote crate
  cargo brief api --crates tokio@1 --features net tokio::net

  # Reduce output verbosity for large crates
  cargo brief api --crates tokio@1 --features full --compact
  cargo brief api --crates tokio@1 --features full --doc-lines 1

RESOLUTION RULES:
  The <TARGET> argument is resolved as follows:
    1. \"self\"           → current package (cwd-based detection)
    2. \"self::mod\"      → current package, specific module
    3. \"crate::mod\"     → named crate + module in one argument
    4. \"src/foo.rs\"     → file path auto-converted to module path
    5. \"crate_name\"     → workspace package (hyphen/underscore normalized)
    6. \"unknown_name\"   → treated as package name (use \"self::mod\" for modules)

  The [MODULE_PATH] argument also accepts file paths (e.g., src/foo.rs).")]
    Api(ApiArgs),

    /// Search for items by name across a crate
    #[command(after_help = "\
EXAMPLES:
  # Search for items by name (smart-case: lowercase=insensitive, uppercase=sensitive)
  cargo brief search --crates axum@0.8 \"Router route\"

  # List all methods/fields of a type
  cargo brief search --crates bytes@1 --methods-of Bytes

  # OR-match with comma: find items matching either term
  cargo brief search self \"EventReader,EventWriter\"

SEARCH MATCHING:
  Smart-case: all-lowercase pattern → case-insensitive, any uppercase → case-sensitive.
  Space-separated words are AND-matched: \"World spawn\" finds items whose path contains both.
  Comma-separated terms are OR-matched: \"Foo,Bar\" finds items matching either.
  Comma splits into OR groups; within each group, spaces are AND.
  Example: \"World spawn,despawn\" matches paths containing (World AND spawn) OR (despawn).

OUTPUT:
  One line per match with full path prefix:
    fn module::Type::method(&self, arg: T) -> Ret;
    struct module::StructName;
    field module::Struct::field_name: Type;
    variant module::Enum::Variant(T1, T2);")]
    Search(SearchArgs),

    /// Grep examples from a crate (not yet implemented)
    Examples(ExamplesArgs),
}

// === Shared Args Groups ===

/// Target resolution arguments (crate + module).
#[derive(Args, Debug, Clone)]
pub struct TargetArgs {
    /// Target to inspect: crate name, "self", crate::module, or file path
    #[arg(value_name = "TARGET", default_value = "self")]
    pub crate_name: String,

    /// Module path or file path within the crate (e.g., "my_mod::submod" or "src/foo.rs")
    pub module_path: Option<String>,

    /// Caller's package name (for visibility resolution)
    #[arg(long, help_heading = "Local Workspace")]
    pub at_package: Option<String>,

    /// Caller's module path (determines what is visible)
    #[arg(long, help_heading = "Local Workspace")]
    pub at_mod: Option<String>,

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,
}

/// Remote crate (crates.io) arguments.
#[derive(Args, Debug, Clone)]
pub struct RemoteArgs {
    /// Fetch a crate from crates.io (e.g., serde, tokio@1, quinn@0.11.0)
    #[arg(long, value_name = "SPEC", help_heading = "Remote Crate (crates.io)")]
    pub crates: Option<String>,

    /// Comma-separated features to enable (e.g., rt,net,macros)
    #[arg(
        long,
        value_name = "FEATURES",
        help_heading = "Remote Crate (crates.io)"
    )]
    pub features: Option<String>,

    /// Skip cache and use a temporary workspace
    #[arg(long, help_heading = "Remote Crate (crates.io)")]
    pub no_cache: bool,

    /// Clear cached remote crate workspaces. Use alone or with a crate spec.
    #[arg(
        long,
        value_name = "SPEC",
        num_args = 0..=1,
        default_missing_value = "",
        help_heading = "Remote Crate (crates.io)"
    )]
    pub clean: Option<String>,
}

/// Output filtering and density flags.
#[derive(Args, Debug, Clone)]
pub struct FilterArgs {
    /// Exclude structs
    #[arg(long, help_heading = "Filtering")]
    pub no_structs: bool,

    /// Exclude enums
    #[arg(long, help_heading = "Filtering")]
    pub no_enums: bool,

    /// Exclude traits
    #[arg(long, help_heading = "Filtering")]
    pub no_traits: bool,

    /// Exclude free functions
    #[arg(long, help_heading = "Filtering")]
    pub no_functions: bool,

    /// Exclude type aliases
    #[arg(long, help_heading = "Filtering")]
    pub no_aliases: bool,

    /// Exclude constants and statics
    #[arg(long, help_heading = "Filtering")]
    pub no_constants: bool,

    /// Exclude unions
    #[arg(long, help_heading = "Filtering")]
    pub no_unions: bool,

    /// Exclude macros
    #[arg(long, help_heading = "Filtering")]
    pub no_macros: bool,

    /// Suppress doc comments from output
    #[arg(long, help_heading = "Filtering")]
    pub no_docs: bool,

    /// Suppress crate-level //! documentation
    #[arg(long, help_heading = "Filtering")]
    pub no_crate_docs: bool,

    /// Limit doc comments to first N lines (0 = suppress all)
    #[arg(long, value_name = "N", help_heading = "Filtering")]
    pub doc_lines: Option<usize>,

    /// Compact output: suppress doc comments, collapse struct fields, enum variants, and trait items
    #[arg(long, help_heading = "Filtering")]
    pub compact: bool,

    /// Show all attributes (#[must_use], #[repr(...)], etc.)
    #[arg(long, help_heading = "Filtering")]
    pub verbose_metadata: bool,

    /// Show all item kinds including blanket/auto-trait impls
    #[arg(long)]
    pub all: bool,
}

/// Global options shared across all subcommands.
#[derive(Args, Debug, Clone)]
pub struct GlobalArgs {
    /// Nightly toolchain name
    #[arg(long, default_value = "nightly", help_heading = "Advanced")]
    pub toolchain: String,

    /// Show progress messages on stderr during pipeline execution
    #[arg(short, long)]
    pub verbose: bool,
}

// === Subcommand Args ===

/// Arguments for the `api` subcommand.
#[derive(Args, Debug, Clone)]
pub struct ApiArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[command(flatten)]
    pub remote: RemoteArgs,

    #[command(flatten)]
    pub filter: FilterArgs,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// How many submodule levels to recurse into
    #[arg(long, default_value = "1")]
    pub depth: u32,

    /// Recurse into all submodules (no depth limit)
    #[arg(long)]
    pub recursive: bool,

    /// Inline full definitions from glob re-export sources
    #[arg(long)]
    pub expand_glob: bool,
}

/// Arguments for the `search` subcommand.
#[derive(Args, Debug, Clone)]
pub struct SearchArgs {
    /// Target crate to search: crate name, "self", or crate::module
    #[arg(value_name = "TARGET", default_value = "self")]
    pub crate_name: String,

    /// Search pattern (smart-case: all-lowercase=insensitive, any uppercase=sensitive).
    /// Space-separated words = AND, comma-separated terms = OR.
    /// Empty when --methods-of is used alone.
    #[arg(default_value = "")]
    pub pattern: String,

    #[command(flatten)]
    pub remote: RemoteArgs,

    #[command(flatten)]
    pub filter: FilterArgs,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// Caller's package name (for visibility resolution)
    #[arg(long, help_heading = "Local Workspace")]
    pub at_package: Option<String>,

    /// Caller's module path (determines what is visible)
    #[arg(long, help_heading = "Local Workspace")]
    pub at_mod: Option<String>,

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,

    /// Limit search results: N (first N) or OFFSET:N (skip OFFSET, show N)
    #[arg(long, value_name = "[OFFSET:]N")]
    pub limit: Option<String>,

    /// Show methods/fields of a type (shorthand for pattern + exclusion flags)
    #[arg(long, value_name = "TYPE")]
    pub methods_of: Option<String>,
}

/// Arguments for the `examples` subcommand (stub).
#[derive(Args, Debug, Clone)]
pub struct ExamplesArgs {
    /// Target crate
    #[arg(value_name = "TARGET", default_value = "self")]
    pub crate_name: String,

    /// Pattern to grep for
    pub pattern: Option<String>,

    #[command(flatten)]
    pub remote: RemoteArgs,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,

    /// Lines of context around matches
    #[arg(long, default_value = "2")]
    pub context: String,

    /// Include tests/ directory in search
    #[arg(long)]
    pub include_tests: bool,
}
