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
  # Substring search (smart-case: all-lowercase = insensitive)
  cargo brief search --crates axum@0.8 Router route
  cargo brief search --crates bevy ShaderRef Material

  # OR-match with comma, methods-of
  cargo brief search self \"EventReader,EventWriter\"
  cargo brief search --crates bytes@1 --methods-of Bytes

  # Glob, exact match, exclusion
  cargo brief search bevy \"Shader*Ref\"               # * = 0+ chars, ? = 1 char
  cargo brief search bevy \"=Router\"                   # final :: segment only
  cargo brief search bevy -- spawn -test              # -- needed for -prefix args
  cargo brief search bevy \"*Plugin*,*Resource* -test\"

PATTERN SYNTAX:
  Smart-case: all-lowercase → case-insensitive, any uppercase → case-sensitive.
  Space = AND, comma = OR. Multiple args are joined with spaces.

  Operators (per token):
    word     substring — path contains \"word\"
    w*ld     glob — * matches 0+ chars, ? matches 1 char (full-path anchored)
    =Name    exact — final path segment (after last ::) equals \"Name\"
    -term    exclude — remove matches (works with substring, glob, or -=exact)

  Exclusions are global across all OR groups.

OUTPUT:
  fn module::Type::method(&self, arg: T) -> Ret;
  struct module::StructName { .. };
  field module::Struct::field_name: Type;
  variant module::Enum::Variant(T1, T2);")]
    Search(SearchArgs),

    /// Grep example/test/bench source files from a crate
    #[command(after_help = "\
EXAMPLES:
  # List example files with their module docs
  cargo brief examples self
  cargo brief examples --crates tokio@1

  # Grep for a pattern in example files
  cargo brief examples self spawn
  cargo brief examples --crates hecs spawn_at --context 3

  # Multiple patterns are AND-matched (no quotes needed)
  cargo brief examples self spawn async

  # Include tests and benches directories
  cargo brief examples --crates serde --tests --benches derive

MATCHING:
  Multiple pattern arguments are joined with spaces (AND-matched).
  Smart-case: all-lowercase pattern = case-insensitive, any uppercase = case-sensitive.
  Without a pattern, lists files with their //! doc comments.")]
    Examples(ExamplesArgs),

    /// Show a compact module-level summary with item counts
    #[command(after_help = "\
EXAMPLES:
  # Summarize the current crate
  cargo brief summary self

  # Summarize a remote crate
  cargo brief summary --crates tokio@1 --features full

  # Summarize a specific module
  cargo brief summary --crates bevy bevy::ecs

OUTPUT:
  One line per visible module with item counts:
    mod io;          // 4 traits, 15 structs, 8 fns
    mod sync::mpsc;  // 4 structs
    // root: 5 macros, 2 fns")]
    Summary(SummaryArgs),
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
#[command(trailing_var_arg = true)]
pub struct SearchArgs {
    /// Target crate to search: crate name, "self", or crate::module
    #[arg(value_name = "TARGET", default_value = "self")]
    pub crate_name: String,

    /// Search patterns — multiple args are AND-matched (use -- for patterns starting with -)
    #[arg(value_name = "PATTERN", trailing_var_arg = true)]
    pub patterns: Vec<String>,

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

    /// Filter results by item kind (comma-separated: fn, struct, enum, trait, union, field, variant, const, static, type, macro, use)
    #[arg(long, value_name = "KINDS", help_heading = "Filtering")]
    pub search_kind: Option<String>,
}

impl SearchArgs {
    /// Join pattern args with space (AND semantics). Empty string if no patterns.
    pub fn pattern(&self) -> String {
        self.patterns.join(" ")
    }
}

/// Arguments for the `examples` subcommand.
#[derive(Args, Debug, Clone)]
#[command(trailing_var_arg = true)]
pub struct ExamplesArgs {
    /// Target crate
    #[arg(value_name = "TARGET", default_value = "self")]
    pub crate_name: String,

    /// Grep patterns — multiple args are AND-matched (omit for list mode)
    #[arg(value_name = "PATTERN", trailing_var_arg = true)]
    pub patterns: Vec<String>,

    #[command(flatten)]
    pub remote: RemoteArgs,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,

    /// Lines of context around matches: N or BEFORE:AFTER
    #[arg(long, default_value = "2")]
    pub context: String,

    /// Include tests/ directory [default depth: unlimited]
    #[arg(long, num_args(0..=1), default_missing_value = "999", value_name = "DEPTH")]
    pub tests: Option<u32>,

    /// Include benches/ directory [default depth: unlimited]
    #[arg(long, num_args(0..=1), default_missing_value = "999", value_name = "DEPTH")]
    pub benches: Option<u32>,
}

/// Arguments for the `summary` subcommand.
#[derive(Args, Debug, Clone)]
pub struct SummaryArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[command(flatten)]
    pub remote: RemoteArgs,

    #[command(flatten)]
    pub global: GlobalArgs,
}

impl ExamplesArgs {
    /// Join pattern args with space. None if no patterns (list mode).
    pub fn pattern(&self) -> Option<String> {
        if self.patterns.is_empty() {
            None
        } else {
            Some(self.patterns.join(" "))
        }
    }
}
