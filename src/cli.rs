use clap::{Parser, Subcommand};

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
    Brief(BriefArgs),
}

/// Core arguments for cargo-brief.
#[derive(Parser, Debug, Clone)]
#[command(
    version,
    after_help = "\
RESOLUTION RULES:
  The <TARGET> argument is resolved as follows:
    1. \"self\"           → current package (cwd-based detection)
    2. \"self::mod\"      → current package, specific module
    3. \"crate::mod\"     → named crate + module in one argument
    4. \"src/foo.rs\"     → file path auto-converted to module path
    5. \"crate_name\"     → workspace package (hyphen/underscore normalized)
    6. \"unknown_name\"   → treated as package name (use \"self::mod\" for modules)

  The [MODULE_PATH] argument also accepts file paths (e.g., src/foo.rs).

SEARCH MODE:
  --search <PATTERN>  searches all leaf items in the crate by name.
  Matching is case-insensitive substring on the full path (e.g., \"world::World::spawn\").
  Multiple words are AND-matched: \"World spawn\" finds items whose path contains both.

  Output: one line per match with full path prefix:
    fn module::Type::method(&self, arg: T) -> Ret;
    struct module::StructName;
    field module::Struct::field_name: Type;
    variant module::Enum::Variant(T1, T2);
    const module::CONST_NAME: Type = ..;
    type module::AliasName = ActualType;
    macro module::macro_name!;"
)]
pub struct BriefArgs {
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

    /// How many submodule levels to recurse into
    #[arg(long, default_value = "1")]
    pub depth: u32,

    /// Recurse into all submodules (no depth limit)
    #[arg(long)]
    pub recursive: bool,

    /// Show all item kinds including blanket/auto-trait impls
    #[arg(long)]
    pub all: bool,

    /// Inline full definitions from glob re-export sources
    #[arg(long)]
    pub expand_glob: bool,

    /// Search for leaf items by name (case-insensitive substring match on full path).
    /// Multiple words are AND-matched. Outputs one-line-per-item with full path.
    ///
    /// Leaf items: functions, methods, struct fields, enum variants, consts,
    /// statics, type aliases, macros, associated types/consts.
    /// Container types (struct, enum, trait, union) are included when their
    /// name matches directly.
    #[arg(long, value_name = "PATTERN", help_heading = "Search")]
    pub search: Option<String>,

    /// Limit search results: N (first N) or OFFSET:N (skip OFFSET, show N)
    #[arg(long, value_name = "[OFFSET:]N", help_heading = "Search")]
    pub search_limit: Option<String>,

    /// Show methods/fields of a type (shorthand for --search + exclusion flags)
    #[arg(long, value_name = "TYPE", help_heading = "Search")]
    pub methods_of: Option<String>,

    // === Output density flags ===
    /// Suppress doc comments from output
    #[arg(long, help_heading = "Filtering")]
    pub no_docs: bool,

    /// Compact output: suppress doc comments, collapse struct fields, enum variants, and trait items
    #[arg(long, help_heading = "Filtering")]
    pub compact: bool,

    // === Exclusion flags (default: all common items shown) ===
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

    /// Nightly toolchain name
    #[arg(long, default_value = "nightly", help_heading = "Advanced")]
    pub toolchain: String,
}
