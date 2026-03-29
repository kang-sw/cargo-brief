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
    after_help = "Run `cargo brief --help` for AI agent quick guide, or `cargo brief <cmd> --help` for details.",
    after_long_help = "\
QUICK GUIDE — which subcommand for which task:

  \"What's the signature of X?\"        → search [--members]
  \"Show a module's full API surface\"   → api [--depth N] [--compact]
  \"Where is X defined? Show source\"    → code [--refs]
  \"What modules exist in this crate?\"  → summary
  \"How is X used in practice?\"         → examples [--tests]
  \"Find structural patterns in AST\"    → ts '<s-expr>'
  \"Who calls X? All references\"        → lsp references <symbol>
  \"What breaks if I change X?\"         → lsp blast-radius <symbol> [--depth N]
  \"What does X call / who calls X?\"    → lsp call-hierarchy <symbol> [--outgoing]

TYPICAL WORKFLOW:
  cargo brief summary self                    # 1. overview of modules
  cargo brief api self::some_module           # 2. drill into a module
  cargo brief search self SomeType --members  # 3. find a specific item
  cargo brief code fn some_function --refs    # 4. read source + references
  cargo brief lsp references some_function    # 5. cross-crate reference tracking

REMOTE CRATES (-C):
  cargo brief -C summary tokio@1              # explore an unfamiliar crate
  cargo brief -C -F rt,net api tokio@1 net    # feature-gated APIs need -F
  cargo brief -C search serde@1 Serialize     # search a crates.io dependency

TIPS:
  - Feature-gated items are invisible without -F. Try -F full if not found.
  - Smart-case: all-lowercase pattern = case-insensitive, any uppercase = exact.
  - `code` searches ALL workspace members by default (not just current package).
  - `lsp` resolves symbols by name. Workspace items are found instantly;
    external deps (e.g., hecs::World) use usage-site fallback (slower).
    Use unique names; common methods like \"new\" may still be ambiguous.
  - `lsp` spawns a persistent rust-analyzer daemon; first query may be slow
    while indexing. Use `lsp touch` to pre-warm.

Run `cargo brief <subcommand> --help` for subcommand-specific options and examples."
)]
pub struct BriefDirect {
    /// Interpret TARGET as a crates.io package spec (e.g., serde@1, tokio@1.0)
    #[arg(short = 'C', long, global = true)]
    pub crates: bool,

    /// Comma-separated features to enable (requires -C)
    #[arg(
        short = 'F',
        long,
        value_name = "FEATURES",
        global = true,
        requires = "crates"
    )]
    pub features: Option<String>,

    /// Disable default features (requires -C)
    #[arg(long, global = true, requires = "crates")]
    pub no_default_features: bool,

    /// Skip cache and use a temporary workspace (requires -C)
    #[arg(long, global = true, requires = "crates")]
    pub no_cache: bool,

    #[command(subcommand)]
    pub command: BriefCommand,
}

impl BriefDirect {
    /// Extract remote-mode options from the top-level flags.
    pub fn remote_opts(&self) -> RemoteOpts {
        RemoteOpts {
            crates: self.crates,
            features: self.features.clone(),
            no_default_features: self.no_default_features,
            no_cache: self.no_cache,
        }
    }
}

/// Remote crate mode options (extracted from BriefDirect global flags).
#[derive(Debug, Clone, Default)]
pub struct RemoteOpts {
    pub crates: bool,
    pub features: Option<String>,
    pub no_default_features: bool,
    pub no_cache: bool,
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
  cargo brief -C api serde@1 --compact
  cargo brief -C -F rt,net,io-util api tokio@1

  # Browse a specific module of a remote crate
  cargo brief -C api tokio@1::net
  cargo brief -C -F net api tokio@1 net

  # Reduce output verbosity for large crates
  cargo brief -C api tokio@1 --compact
  cargo brief -C api tokio@1 --doc-lines 1

RESOLUTION RULES:
  The <TARGET> argument is resolved as follows:
    1. \"self\"           → current package (cwd-based detection)
    2. \"self::mod\"      → current package, specific module
    3. \"crate::mod\"     → named crate + module in one argument
    4. \"src/foo.rs\"     → file path auto-converted to module path
    5. \"crate_name\"     → workspace package (hyphen/underscore normalized)
    6. \"unknown_name\"   → treated as package name (use \"self::mod\" for modules)

  With -C, TARGET is the crate spec (e.g., serde@1, tokio@1.0).

  The [MODULE_PATH] argument also accepts file paths (e.g., src/foo.rs).")]
    Api(ApiArgs),

    /// Search for items by name across a crate
    #[command(after_help = "\
EXAMPLES:
  # Substring search (smart-case: all-lowercase = insensitive)
  cargo brief -C search axum@0.8 Router route
  cargo brief -C search bevy ShaderRef Material

  # OR-match with comma, methods-of
  cargo brief search self \"EventReader,EventWriter\"
  cargo brief -C search bytes@1 --methods-of Bytes

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

  Exclusions are global across all OR groups.")]
    Search(SearchArgs),

    /// Grep example/test/bench source files from a crate
    #[command(after_help = "\
EXAMPLES:
  # List example files with their module docs
  cargo brief examples self
  cargo brief -C examples tokio@1

  # Grep for a pattern in example files
  cargo brief examples self spawn
  cargo brief -C examples hecs spawn_at --context 3

  # Multiple patterns are AND-matched (no quotes needed)
  cargo brief examples self spawn async

  # Include tests and benches directories
  cargo brief -C examples serde --tests --benches derive

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
  cargo brief -C -F full summary tokio@1

  # Summarize a specific module
  cargo brief -C summary bevy bevy::ecs

RESOLUTION RULES:
  The <TARGET> argument is resolved as follows:
    1. \"self\"           → current package (cwd-based detection)
    2. \"self::mod\"      → current package, specific module
    3. \"crate::mod\"     → named crate + module in one argument
    4. \"src/foo.rs\"     → file path auto-converted to module path
    5. \"crate_name\"     → workspace package (hyphen/underscore normalized)
    6. \"unknown_name\"   → treated as package name

  With -C, TARGET is the crate spec (e.g., serde@1, tokio@1.0).

  The [MODULE_PATH] argument also accepts file paths (e.g., src/foo.rs).")]
    Summary(SummaryArgs),

    /// Run a tree-sitter structural query against crate source files
    #[command(after_help = "\
EXAMPLES:
  # Find all function definitions
  cargo brief ts self '(function_item)'

  # Capture function names
  cargo brief ts self '(function_item name: (identifier) @name)' --captures

  # Find impl blocks for a specific trait
  cargo brief ts self '(impl_item trait: (type_identifier) @t (#eq? @t \"MyTrait\"))'

  # Find struct definitions with their fields
  cargo brief ts self '(struct_item name: (type_identifier) @name body: (field_declaration_list) @fields)' --captures

  # Find functions returning Result
  cargo brief ts self '(function_item return_type: (generic_type type: (type_identifier) @r (#eq? @r \"Result\")))'

  # Find let bindings with type annotations
  cargo brief ts self '(let_declaration pattern: (_) @pat type: (_) @type)' --captures

  # Find all call expressions to a specific function
  cargo brief ts self '(call_expression function: (identifier) @fn (#eq? @fn \"spawn\"))'

  # Location-only output for large result sets
  cargo brief ts self '(function_item)' --quiet

  # Limit results
  cargo brief ts self '(function_item)' --limit 10

  # Only search src/ (skip tests/examples/benches)
  cargo brief ts self '(function_item)' --src-only

  # Query a remote crate's source
  cargo brief -C ts serde@1 '(struct_item)'

QUERY SYNTAX:
  Tree-sitter S-expression patterns match AST nodes by type and structure.
  Explore the AST interactively: https://tree-sitter.github.io/tree-sitter/playground

NODE TYPES (common Rust constructs):
  Items:       function_item, struct_item, enum_item, impl_item, trait_item,
               type_item, const_item, static_item, macro_definition,
               use_declaration, mod_item
  Expressions: call_expression, method_call_expression, field_expression,
               match_expression, if_expression, closure_expression,
               return_expression, await_expression
  Statements:  let_declaration, expression_statement, assignment_expression
  Types:       type_identifier, generic_type, reference_type, array_type,
               function_type, tuple_type
  Other:       attribute_item, line_comment, block_comment, string_literal,
               identifier, field_identifier

CAPTURES:
  @name binds a node. In default mode, the outermost matched node is shown
  (captures used only in predicates don't affect output). With --captures,
  each @name: text pair is shown separately. Capture-less queries work too —
  an internal @_match capture is auto-added.

PREDICATES:
  (#eq? @cap \"value\")     — exact string match on captured node text
  (#match? @cap \"regex\")  — regex match on captured node text
  (#not-eq? @cap \"value\") — negated exact match
  (#any-of? @cap \"a\" \"b\") — match any of the listed strings

TIPS:
  - Rust async functions are function_item with modifiers, not a separate type
  - Use (type_identifier) for type names, (identifier) for variable/fn names
  - Nested captures: (struct_item (field_declaration_list
      (field_declaration name: (field_identifier) @field)))
  - Wildcards: (_) matches any single node type
  - #[derive(...)] is: (attribute_item (attribute (identifier) @a (#eq? @a \"derive\")))")]
    Ts(TsArgs),

    /// Look up code definitions by kind and name using pre-crafted tree-sitter queries
    #[command(after_help = "\
EXAMPLES:
  # Search current workspace (TARGET omitted — defaults to \"self\")
  cargo brief code spawn
  cargo brief code struct Commands

  # Search a specific crate in the workspace
  cargo brief code my-crate fn spawn
  cargo brief code my-crate struct Commands

  # Remote crate (TARGET required with -C)
  cargo brief -C code serde@1 struct Serializer

  # Smart-case: all-lowercase = case-insensitive
  cargo brief code pubstruct              # finds PubStruct
  cargo brief code PubStruct              # case-sensitive

  # Show definitions + references
  cargo brief code fn spawn --refs

  # References only
  cargo brief code spawn --refs-only

  # Scope to items inside a type
  cargo brief code --in Commands fn new
  cargo brief code --in PubStruct field

  # Quiet mode (location only, no source text)
  cargo brief code fn spawn -q

  # Limit results
  cargo brief code fn spawn --limit 5

POSITIONAL ARGS:
  1 arg:   code NAME                  search all workspace members, all kinds
           (error if NAME is a kind keyword — use 2-arg form instead)
  2 args:  code KIND NAME             if first arg is a kind keyword (see below)
           code TARGET NAME           otherwise: search named crate, all kinds
  3 args:  code TARGET KIND NAME      search named crate, filter by kind

  Disambiguation: if the first of two args matches a kind keyword, it is
  treated as KIND (not TARGET). Use the 3-arg form to force a target that
  happens to shadow a kind keyword.

TARGET RESOLUTION:
  \"self\" (default when TARGET omitted):
    Searches ALL workspace members — not just the current package.
    This differs from other subcommands where \"self\" = current package,
    because code is a source-level lookup tool where project-wide search
    is the common case.
  Named crate:
    Searches only that specific package (hyphen/underscore normalized).
  With -C (remote):
    TARGET is a crates.io spec (e.g., serde@1). Implicit \"self\" is not
    allowed — you must provide an explicit TARGET.

DEPENDENCY SEARCH:
  Default:    workspace members + accessible dependencies (via rustdoc JSON
              reachability analysis; requires nightly).
  --no-deps:  workspace members (or named target) only.
  --all-deps: workspace members + all direct dependencies (via cargo
              metadata; no nightly needed, wider but noisier).

ITEM KINDS:
  fn, struct, enum, trait, field, type, impl, macro, const, use
  Omit KIND to search all kinds (except use, to reduce noise).

OUTPUT FORMAT:
  Each match is printed as:
    @<file>:<line>                          — source location
      in <crate>::<module>[, <parent>]      — module path + parent context
                                              (e.g., impl Type, trait Trait)
    <source text>                           — full item definition

  With --quiet (-q), only the location and module-path lines are shown.

NAME MATCHING:
  Smart-case: all-lowercase pattern = case-insensitive, any uppercase =
  case-sensitive. The name must match the item's identifier exactly (not
  a substring).

REFERENCE SEARCH (--refs, --refs-only):
  After definitions, grep for literal name occurrences across the same
  source files. Output uses * markers on match lines with 2 lines of
  surrounding context. --refs-only skips definitions entirely.
  --limit applies to definitions (--refs) or grep matches (--refs-only).

PARENT SCOPING (--in <TYPE>):
  Filter definitions to items inside a specific type, impl block, or
  trait. Matches the type identifier with smart-case rules.
    --in Commands    matches impl Commands, impl T for Commands, etc.
    --in commands    case-insensitive match
  Top-level items (not inside any type) are excluded.")]
    Code(CodeArgs),

    /// Clear cached remote crate workspaces
    #[command(after_help = "\
EXAMPLES:
  # Clear all cached workspaces
  cargo brief clean

  # Clear caches for a specific crate
  cargo brief clean serde")]
    Clean(CleanArgs),

    /// Manage LSP daemon (persistent rust-analyzer)
    #[command(after_help = "\
EXAMPLES:
  # Find all references to a symbol across the workspace
  cargo brief lsp references resolve_symbol
  cargo brief lsp references Foo::bar -q        # quiet: locations only

  # Blast radius: direct + transitive callers (\"what breaks if I change X?\")
  cargo brief lsp blast-radius handle_request
  cargo brief lsp blast-radius handle_request --depth 3

  # Call hierarchy: who calls X (incoming) / what does X call (outgoing)
  cargo brief lsp call-hierarchy spawn
  cargo brief lsp call-hierarchy spawn --outgoing -q

  # Daemon lifecycle (daemon auto-starts on first query)
  cargo brief lsp touch                         # pre-warm rust-analyzer
  cargo brief lsp status                        # check if running
  cargo brief lsp stop                          # shut down daemon

SYMBOL RESOLUTION:
  Symbols are resolved in two stages:
    1. workspace/symbol search (fast, finds workspace-defined items)
    2. Fallback: grep workspace source for usage sites, then resolve via
       textDocument/definition (slower, finds external deps like hecs::World)

  Tips:
    - Qualified names work: \"hecs::World\", \"App::new\", \"MyStruct::method\"
    - Common names like \"new\" or \"get\" may return Ambiguous (many matches).
      Use a qualified form to narrow down.
    - call-hierarchy and blast-radius work on functions/methods, not types.
      Use `references` for tracking struct/enum/trait usage.
    - If resolution still fails, try `code --refs <name>` as a last resort.

NOTE:
  The LSP daemon spawns automatically on first query. Initial indexing may
  take time; subsequent queries are fast. Use `lsp touch` to pre-warm.")]
    Lsp(LspArgs),
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
    pub filter: FilterArgs,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// How many submodule levels to recurse into
    #[arg(long, default_value = "1")]
    pub depth: u32,

    /// Recurse into all submodules (no depth limit)
    #[arg(long)]
    pub recursive: bool,

    /// Suppress glob re-export expansion (show pub use lines instead of inlined definitions)
    #[arg(long)]
    pub no_expand_glob: bool,
}

/// Arguments for the `search` subcommand.
#[derive(Args, Debug, Clone)]
pub struct SearchArgs {
    /// Target crate to search: crate name, "self", or crate::module
    #[arg(value_name = "TARGET", default_value = "self")]
    pub crate_name: String,

    /// Search patterns — multiple args are AND-matched (use -- for patterns starting with -)
    #[arg(value_name = "PATTERN", num_args = 0..)]
    pub patterns: Vec<String>,

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

    /// Show all members (fields, variants, methods) of matched types
    #[arg(long)]
    pub members: bool,
}

impl SearchArgs {
    /// Join pattern args with space (AND semantics). Empty string if no patterns.
    pub fn pattern(&self) -> String {
        self.patterns.join(" ")
    }
}

/// Arguments for the `examples` subcommand.
#[derive(Args, Debug, Clone)]
pub struct ExamplesArgs {
    /// Target crate: crate name, "self", or crates.io spec (with -C)
    #[arg(value_name = "TARGET", default_value = "self")]
    pub crate_name: String,

    /// Grep patterns — multiple args are AND-matched (omit for list mode)
    #[arg(value_name = "PATTERN", num_args = 0..)]
    pub patterns: Vec<String>,

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

/// Arguments for the `ts` (tree-sitter) subcommand.
#[derive(Args, Debug, Clone)]
pub struct TsArgs {
    /// Target crate (use 'self' for current crate)
    #[arg(value_name = "TARGET")]
    pub crate_name: String,

    /// Tree-sitter S-expression query
    #[arg(value_name = "QUERY")]
    pub query: String,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,

    /// Show capture names with matched text (for multi-capture queries)
    #[arg(long)]
    pub captures: bool,

    /// Lines of context around matched nodes: N or BEFORE:AFTER
    #[arg(long, default_value = "0")]
    pub context: String,

    /// Only search src/ directory (skip examples, tests, benches)
    #[arg(long)]
    pub src_only: bool,

    /// Limit matches: N (first N) or OFFSET:N (skip OFFSET, show N)
    #[arg(long, value_name = "[OFFSET:]N")]
    pub limit: Option<String>,

    /// Output only file:line locations, no source text
    #[arg(short = 'q', long)]
    pub quiet: bool,
}

/// Arguments for the `code` subcommand.
#[derive(Args, Debug, Clone)]
pub struct CodeArgs {
    /// Positional arguments: [TARGET] [KIND] NAME
    #[arg(value_name = "ARGS", num_args = 1..=3)]
    pub args: Vec<String>,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,

    /// Only search src/ directory (skip examples, tests, benches)
    #[arg(long)]
    pub src_only: bool,

    /// Don't search dependencies (target crate only)
    #[arg(long)]
    pub no_deps: bool,

    /// Search all direct dependencies (no nightly needed; skips accessible-path filtering)
    #[arg(long, conflicts_with = "no_deps")]
    pub all_deps: bool,

    /// Limit matches: N (first N) or OFFSET:N (skip OFFSET, show N)
    #[arg(long, value_name = "[OFFSET:]N")]
    pub limit: Option<String>,

    /// Output only file:line locations and module context, no source text
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Also show grep-based references after definitions
    #[arg(long)]
    pub refs: bool,

    /// Only show grep references, skip definitions
    #[arg(long, conflicts_with = "refs")]
    pub refs_only: bool,

    /// Scope to items inside a specific type/impl block
    #[arg(long, value_name = "TYPE")]
    pub in_type: Option<String>,
}

/// Arguments for the `summary` subcommand.
#[derive(Args, Debug, Clone)]
pub struct SummaryArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[command(flatten)]
    pub global: GlobalArgs,
}

/// Arguments for the `clean` subcommand.
#[derive(Args, Debug, Clone)]
pub struct CleanArgs {
    /// Crate spec to clean (omit to clean all)
    #[arg(value_name = "SPEC")]
    pub spec: Option<String>,
}

/// Arguments for the `lsp` subcommand.
#[derive(Args, Debug, Clone)]
pub struct LspArgs {
    #[command(subcommand)]
    pub command: LspCommand,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum LspCommand {
    /// Ensure LSP daemon is running (pre-warm rust-analyzer)
    Touch {
        /// Return immediately after ensuring daemon is running (skip indexing wait)
        #[arg(long)]
        no_wait: bool,
    },
    /// Stop the LSP daemon
    Stop,
    /// Show LSP daemon status
    Status,
    /// Find all references to a symbol via rust-analyzer
    References {
        /// Symbol to find references for (e.g., "Foo::bar", "CrateModel")
        symbol: String,
        /// Location-only output format
        #[arg(long, short)]
        quiet: bool,
    },
    /// Show direct and transitive callers of a symbol (blast radius)
    BlastRadius {
        /// Symbol to analyze (e.g., "resolve_symbol", "Foo::bar")
        symbol: String,
        /// Depth of transitive caller search (1 = direct only, max 10)
        #[arg(long, default_value = "1")]
        depth: u32,
        /// Location-only output format
        #[arg(long, short)]
        quiet: bool,
    },
    /// Show incoming or outgoing call hierarchy for a symbol
    CallHierarchy {
        /// Symbol to analyze (e.g., "resolve_symbol", "Foo::bar")
        symbol: String,
        /// Show outgoing calls instead of incoming
        #[arg(long)]
        outgoing: bool,
        /// Location-only output format
        #[arg(long, short)]
        quiet: bool,
    },
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
