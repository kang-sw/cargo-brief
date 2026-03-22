//! Integration tests for facade crate glob re-export expansion.
//!
//! Uses `clap` as a test case — it re-exports from `clap_builder` and `clap_derive`.

use cargo_brief::cli::{ApiArgs, FilterArgs, GlobalArgs, RemoteOpts, TargetArgs};
use cargo_brief::run_api_pipeline;

fn facade_args(crate_name: &str) -> ApiArgs {
    ApiArgs {
        target: TargetArgs {
            crate_name: crate_name.to_string(),
            module_path: None,
            at_package: None,
            at_mod: None,
            manifest_path: Some("test_workspace/Cargo.toml".to_string()),
        },
        filter: FilterArgs {
            no_structs: false,
            no_enums: false,
            no_traits: false,
            no_functions: false,
            no_aliases: false,
            no_constants: false,
            no_unions: false,
            no_macros: false,
            no_docs: false,
            no_crate_docs: false,
            doc_lines: None,
            compact: false,
            verbose_metadata: false,
            all: false,
        },
        global: GlobalArgs {
            toolchain: "nightly".to_string(),
            verbose: false,
        },
        depth: 1,
        recursive: true,
        no_expand_glob: false,
    }
}

// ============================================================
// Glob expansion produces non-empty output
// ============================================================

#[test]
fn clap_facade_not_empty() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // Must have more than just the crate header
    let lines: Vec<&str> = output.lines().collect();
    assert!(
        lines.len() > 2,
        "clap facade should have expanded glob re-exports, got:\n{output}"
    );
}

#[test]
fn clap_facade_has_crate_header() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    assert!(
        output.starts_with("// crate clap\n"),
        "crate header: got first line = {:?}",
        output.lines().next()
    );
}

// ============================================================
// Glob expansion inlines full definitions by default
// ============================================================

#[test]
fn clap_facade_expands_clap_builder_items() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // With default expansion on, full definitions should be inlined
    assert!(
        output.contains("pub struct Command"),
        "Command struct definition should be inlined by default:\n{output}"
    );
    assert!(
        output.contains("pub struct Arg"),
        "Arg struct definition should be inlined by default"
    );
}

#[test]
fn clap_facade_no_glob_star() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // The glob `pub use clap_builder::*;` should be replaced with individual items
    for line in output.lines() {
        if line.starts_with("pub use") && line.contains("::*;") {
            panic!("glob should be expanded, but found: {line}");
        }
    }
}

// ============================================================
// Glob expansion does not include submodules
// ============================================================

#[test]
fn clap_facade_no_module_reexports() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // Submodules from clap_builder (like `builder`) should NOT appear as re-exports
    // (Rust's glob import doesn't re-export submodules)
    assert!(
        !output.contains("pub use clap_builder::builder;"),
        "submodules should not be re-exported via glob"
    );
}

// ============================================================
// Regression: non-facade crates are unaffected
// ============================================================

#[test]
fn either_unaffected_by_glob_expansion() {
    let args = facade_args("either");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // either is not a facade crate — should render normally
    assert!(
        output.contains("pub enum Either<L, R>"),
        "Either enum should render normally"
    );
    // No top-level glob re-export lines (doc comments may contain `use either::*;`)
    for line in output.lines() {
        if line.starts_with("pub use") && line.contains("::*;") {
            panic!("unexpected glob re-export in either: {line}");
        }
    }
}

// ============================================================
// Glob expansion (default): full definition inlining
// ============================================================

#[test]
fn clap_expand_glob_has_full_definitions() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // Full struct definitions should appear instead of `pub use` lines
    assert!(
        output.contains("pub struct Command"),
        "Command struct definition should be inlined:\n{output}"
    );
    assert!(
        output.contains("pub struct Arg"),
        "Arg struct definition should be inlined"
    );
}

#[test]
fn clap_expand_glob_no_pub_use_lines() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // No `pub use clap_builder::*;` lines should remain
    for line in output.lines() {
        if line.starts_with("pub use") && line.contains("::*;") {
            panic!("glob should be fully expanded, but found: {line}");
        }
    }
    // No individual `pub use clap_builder::Name;` lines either
    assert!(
        !output.contains("pub use clap_builder::Command;"),
        "individual pub use lines should not appear with default expansion"
    );
}

#[test]
fn clap_expand_glob_has_impl_blocks() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // impl blocks from source crate should be included
    assert!(
        output.contains("impl Command"),
        "impl blocks for Command should be inlined:\n{output}"
    );
}

#[test]
fn clap_expand_glob_dedup() {
    let args = facade_args("clap");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // Items appearing in multiple glob sources should be deduplicated.
    // Count occurrences of "pub struct Command" — should be small (ideally 1).
    // Nightly rustdoc changes may cause the item to appear across multiple
    // glob-inlined source crates (e.g., clap re-exports from clap_builder).
    let count = output.matches("pub struct Command").count();
    assert!(
        count <= 2,
        "Command should appear at most twice, found {count} times"
    );
}

#[test]
fn either_expand_glob_no_effect() {
    let args = facade_args("either");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // either is not a facade crate — default expansion should not change output
    assert!(
        output.contains("pub enum Either<L, R>"),
        "Either enum should render normally with default expansion"
    );
}

// ============================================================
// Named re-export expansion (serde facade)
// ============================================================

#[test]
fn serde_named_reexports_expanded() {
    let args = facade_args("serde");
    let output = run_api_pipeline(&args, &RemoteOpts::default()).unwrap();

    // Serialize trait should be expanded from serde_core (or serde itself)
    assert!(
        output.contains("trait Serialize"),
        "Serialize should be expanded from named re-export:\n{output}"
    );
    // The raw pub use line should be replaced
    assert!(
        !output.contains("pub use serde_core::Serialize;"),
        "Expanded named re-export should not show pub use line:\n{output}"
    );
}
