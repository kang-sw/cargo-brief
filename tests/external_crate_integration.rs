//! Integration tests for external (non-workspace) crate support.
//!
//! Uses `either =1.15.0` as a pinned external dependency in test_workspace/core-lib.
//! These tests verify that cargo-brief can generate correct output for crates
//! that are not workspace members.

use cargo_brief::cli::BriefArgs;
use cargo_brief::run_pipeline;

fn either_args() -> BriefArgs {
    BriefArgs {
        crate_name: "either".to_string(),
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
        toolchain: "nightly".to_string(),
        manifest_path: Some("test_workspace/Cargo.toml".to_string()),
    }
}

// ============================================================
// Basic structure
// ============================================================

#[test]
fn either_crate_header() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.starts_with("// crate either\n"),
        "crate header: got first line = {:?}",
        output.lines().next()
    );
}

#[test]
fn either_enum_definition() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub enum Either<L, R>"),
        "Either enum definition"
    );
    assert!(output.contains("Left(L),"), "Left variant");
    assert!(output.contains("Right(R),"), "Right variant");
}

#[test]
fn either_enum_doc_comment() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("/// The enum `Either` with variants `Left` and `Right`"),
        "Either enum doc comment"
    );
}

// ============================================================
// Inherent impl methods
// ============================================================

#[test]
fn either_has_core_methods() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(output.contains("pub fn is_left(&self) -> bool;"), "is_left");
    assert!(
        output.contains("pub fn is_right(&self) -> bool;"),
        "is_right"
    );
    assert!(output.contains("pub fn left(self) -> Option<L>;"), "left()");
    assert!(
        output.contains("pub fn right(self) -> Option<R>;"),
        "right()"
    );
    assert!(
        output.contains("pub fn flip(self) -> Either<R, L>;"),
        "flip"
    );
    assert!(
        output.contains("pub fn unwrap_left(self) -> L;"),
        "unwrap_left"
    );
    assert!(
        output.contains("pub fn unwrap_right(self) -> R;"),
        "unwrap_right"
    );
}

#[test]
fn either_has_map_methods() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub fn map_left<F, M>(self, f: F) -> Either<M, R>;"),
        "map_left"
    );
    assert!(
        output.contains("pub fn map_right<F, S>(self, f: F) -> Either<L, S>;"),
        "map_right"
    );
}

#[test]
fn either_has_into_inner_for_same_type() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(output.contains("impl<T> Either<T, T>"), "impl Either<T, T>");
    assert!(
        output.contains("pub fn into_inner(self) -> T;"),
        "into_inner"
    );
}

// ============================================================
// Trait: IntoEither
// ============================================================

#[test]
fn either_into_either_trait() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    // IntoEither is in a pub(crate) module, but re-exported at root
    assert!(
        output.contains("pub use self::into_either::IntoEither"),
        "IntoEither re-export at root"
    );
    // The trait definition itself is hidden (pub(crate) module) in cross-crate view
    assert!(
        !output.contains("pub trait IntoEither: Sized"),
        "IntoEither trait definition should be hidden in cross-crate view"
    );
}

// ============================================================
// Struct: IterEither
// ============================================================

#[test]
fn either_iter_either_struct() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    // IterEither is in a pub(crate) module, but re-exported at root
    assert!(
        output.contains("pub use self::iterator::IterEither"),
        "IterEither re-export at root"
    );
    // The struct definition itself is hidden (pub(crate) module) in cross-crate view
    assert!(
        !output.contains("pub struct IterEither<L, R>"),
        "IterEither struct should be hidden in cross-crate view"
    );
}

// ============================================================
// Re-exports
// ============================================================

#[test]
fn either_reexports() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub use") && output.contains("IterEither"),
        "IterEither re-export"
    );
    assert!(
        output.contains("pub use") && output.contains("IntoEither"),
        "IntoEither re-export"
    );
}

// ============================================================
// Trait impls
// ============================================================

#[test]
fn either_trait_impls() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    // Iterator impl with associated type
    assert!(
        output.contains("impl<L, R> Iterator for") && output.contains("Either<L, R>"),
        "Iterator impl for Either"
    );

    // Clone impl
    assert!(
        output.contains("Clone for Either<L, R>"),
        "Clone impl for Either"
    );

    // From<Result> conversion
    assert!(
        output.contains("impl<L, R> From<Result<R, L>> for Either<L, R>"),
        "From<Result> impl"
    );
}

#[test]
fn either_deref_impl() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("impl<L, R> Deref for Either<L, R>"),
        "Deref impl"
    );
}

// ============================================================
// Submodule structure
// ============================================================

#[test]
fn either_hides_pub_crate_modules() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    // pub(crate) modules are hidden in cross-crate view
    assert!(
        !output.contains("mod iterator {"),
        "iterator module should be hidden in cross-crate view"
    );
    assert!(
        !output.contains("mod into_either {"),
        "into_either module should be hidden in cross-crate view"
    );
}

// ============================================================
// Module targeting
// ============================================================

#[test]
fn either_target_iterator_module() {
    let mut args = either_args();
    args.module_path = Some("iterator".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub struct IterEither<L, R>"),
        "IterEither in iterator module"
    );
    // Should not show root-level Either enum
    assert!(
        !output.contains("pub enum Either<L, R>"),
        "Either enum should not appear in iterator module view"
    );
}

// ============================================================
// Macros
// ============================================================

#[test]
fn either_has_macros() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(output.contains("macro_rules! for_both"), "for_both macro");
    assert!(output.contains("macro_rules! try_left"), "try_left macro");
    assert!(output.contains("macro_rules! try_right"), "try_right macro");
}

// ============================================================
// Depth control
// ============================================================

#[test]
fn either_depth_zero_still_shows_root_items() {
    let mut args = either_args();
    args.recursive = false;
    args.depth = 0;
    let output = run_pipeline(&args).unwrap();

    // pub(crate) modules are hidden in cross-crate view, even at depth 0
    assert!(
        !output.contains("mod iterator"),
        "iterator module hidden in cross-crate view"
    );
    assert!(
        !output.contains("mod into_either"),
        "into_either module hidden in cross-crate view"
    );
    // Root-level items still present
    assert!(
        output.contains("pub enum Either<L, R>"),
        "Either enum at depth 0"
    );
}

// ============================================================
// Doc comments on methods
// ============================================================

#[test]
fn either_method_doc_comments() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("/// Return true if the value is the `Left` variant."),
        "is_left doc comment"
    );
    assert!(
        output.contains("/// Convert `Either<L, R>` to `Either<R, L>`."),
        "flip doc comment"
    );
}

// ============================================================
// Versioned package specifier (pkg@version)
// ============================================================

#[test]
fn either_versioned_specifier() {
    let mut args = either_args();
    args.crate_name = "either@1.15.0".to_string();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.starts_with("// crate either\n"),
        "crate header with versioned specifier: got first line = {:?}",
        output.lines().next()
    );
    assert!(
        output.contains("pub enum Either<L, R>"),
        "Either enum with versioned specifier"
    );
}

// ============================================================
// Specialized impls (Option/Result factoring)
// ============================================================

#[test]
fn either_specialized_impls() {
    let args = either_args();
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("impl<L, R> Either<Option<L>, Option<R>>"),
        "Option factoring impl"
    );
    assert!(
        output.contains("pub fn factor_none(self) -> Option<Either<L, R>>;"),
        "factor_none method"
    );
}
