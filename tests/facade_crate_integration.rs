//! Integration tests for facade crate glob re-export expansion.
//!
//! Uses `clap` as a test case — it re-exports from `clap_builder` and `clap_derive`.

use cargo_brief::cli::BriefArgs;
use cargo_brief::run_pipeline;

fn facade_args(crate_name: &str) -> BriefArgs {
    BriefArgs {
        crate_name: crate_name.to_string(),
        module_path: None,
        at_package: None,
        at_mod: None,
        depth: 1,
        recursive: true,
        all: false,
        no_structs: false,
        no_enums: false,
        no_traits: false,
        no_functions: false,
        no_aliases: false,
        no_constants: false,
        no_unions: false,
        no_macros: false,
        crates: None,
        expand_glob: false,
        no_cache: false,
        toolchain: "nightly".to_string(),
        manifest_path: Some("test_workspace/Cargo.toml".to_string()),
    }
}

// ============================================================
// Glob expansion produces non-empty output
// ============================================================

#[test]
fn clap_facade_not_empty() {
    let args = facade_args("clap");
    let output = run_pipeline(&args).unwrap();

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
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.starts_with("// crate clap\n"),
        "crate header: got first line = {:?}",
        output.lines().next()
    );
}

// ============================================================
// Glob expansion shows individual pub use items
// ============================================================

#[test]
fn clap_facade_expands_clap_builder_items() {
    let args = facade_args("clap");
    let output = run_pipeline(&args).unwrap();

    // Key types from clap_builder should appear as individual pub use
    assert!(
        output.contains("pub use clap_builder::Command;"),
        "Command should be re-exported from clap_builder:\n{output}"
    );
    assert!(
        output.contains("pub use clap_builder::Arg;"),
        "Arg should be re-exported from clap_builder"
    );
}

#[test]
fn clap_facade_no_glob_star() {
    let args = facade_args("clap");
    let output = run_pipeline(&args).unwrap();

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
    let output = run_pipeline(&args).unwrap();

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
    let output = run_pipeline(&args).unwrap();

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
// --expand-glob: full definition inlining
// ============================================================

fn expand_glob_args(crate_name: &str) -> BriefArgs {
    let mut args = facade_args(crate_name);
    args.expand_glob = true;
    args
}

#[test]
fn clap_expand_glob_has_full_definitions() {
    let args = expand_glob_args("clap");
    let output = run_pipeline(&args).unwrap();

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
    let args = expand_glob_args("clap");
    let output = run_pipeline(&args).unwrap();

    // No `pub use clap_builder::*;` lines should remain
    for line in output.lines() {
        if line.starts_with("pub use") && line.contains("::*;") {
            panic!("glob should be fully expanded, but found: {line}");
        }
    }
    // No individual `pub use clap_builder::Name;` lines either
    assert!(
        !output.contains("pub use clap_builder::Command;"),
        "individual pub use lines should not appear with --expand-glob"
    );
}

#[test]
fn clap_expand_glob_has_impl_blocks() {
    let args = expand_glob_args("clap");
    let output = run_pipeline(&args).unwrap();

    // impl blocks from source crate should be included
    assert!(
        output.contains("impl Command"),
        "impl blocks for Command should be inlined:\n{output}"
    );
}

#[test]
fn clap_expand_glob_dedup() {
    let args = expand_glob_args("clap");
    let output = run_pipeline(&args).unwrap();

    // Items appearing in multiple glob sources should be rendered only once.
    // Count occurrences of "pub struct Command" — should be exactly 1.
    let count = output.matches("pub struct Command").count();
    assert!(
        count <= 1,
        "Command should appear at most once, found {count} times"
    );
}

#[test]
fn either_expand_glob_no_effect() {
    let args = expand_glob_args("either");
    let output = run_pipeline(&args).unwrap();

    // either is not a facade crate — --expand-glob should not change output
    assert!(
        output.contains("pub enum Either<L, R>"),
        "Either enum should render normally with --expand-glob"
    );
}
