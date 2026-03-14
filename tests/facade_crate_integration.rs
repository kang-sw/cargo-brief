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
