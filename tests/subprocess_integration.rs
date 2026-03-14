//! Subprocess-based integration tests for cargo-brief.
//!
//! These tests invoke the `cargo-brief` binary via `std::process::Command` with
//! explicit working directories and arguments. This exercises the full pipeline
//! including cwd detection, `self` resolution, and arg parsing — things that
//! in-process tests via `run_pipeline()` cannot cover.
//!
//! Test fixture: `test_workspace/` (workspace with `core-lib` + `app` crates,
//! `either` as an external dependency of `core-lib`).

use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn cargo_brief_bin() -> PathBuf {
    env!("CARGO_BIN_EXE_cargo-brief").into()
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn test_workspace() -> PathBuf {
    project_root().join("test_workspace")
}

/// Run cargo-brief binary from the given `cwd` with `args`.
/// Returns `(stdout, stderr, success)`.
fn run(cwd: &Path, args: &[&str]) -> (String, String, bool) {
    let output = Command::new(cargo_brief_bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("Failed to execute cargo-brief");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Run and assert success, return stdout.
fn run_ok(cwd: &Path, args: &[&str]) -> String {
    let (stdout, stderr, success) = run(cwd, args);
    assert!(
        success,
        "Expected success but got failure.\nArgs: {args:?}\nStderr:\n{stderr}"
    );
    stdout
}

/// Run and assert failure, return stderr.
fn run_err(cwd: &Path, args: &[&str]) -> String {
    let (stdout, stderr, success) = run(cwd, args);
    assert!(
        !success,
        "Expected failure but got success.\nArgs: {args:?}\nStdout:\n{stdout}"
    );
    stderr
}

// ===========================================================================
// A. Explicit crate name (from workspace root)
// ===========================================================================

#[test]
fn explicit_core_lib() {
    let out = run_ok(&test_workspace(), &["core-lib"]);
    assert!(out.contains("pub struct Config"), "missing Config struct");
    assert!(
        out.contains("pub trait Processor"),
        "missing Processor trait"
    );
}

#[test]
fn explicit_app() {
    let out = run_ok(&test_workspace(), &["app"]);
    assert!(out.contains("pub struct App"), "missing App struct");
}

#[test]
fn explicit_underscore_normalization() {
    let out = run_ok(&test_workspace(), &["core_lib"]);
    assert!(
        out.contains("pub struct Config"),
        "underscore normalization failed — missing Config struct"
    );
}

// ===========================================================================
// B. `self` keyword (cwd-dependent)
// ===========================================================================

#[test]
fn self_from_core_lib() {
    let out = run_ok(&test_workspace().join("core-lib"), &["self"]);
    assert!(out.contains("pub struct Config"), "missing Config struct");
}

#[test]
fn self_from_app() {
    let out = run_ok(&test_workspace().join("app"), &["self"]);
    assert!(out.contains("pub struct App"), "missing App struct");
}

#[test]
fn self_module_from_core_lib() {
    let out = run_ok(&test_workspace().join("core-lib"), &["self::utils"]);
    assert!(
        out.contains("pub fn format_name"),
        "missing format_name in utils module"
    );
}

#[test]
fn self_from_virtual_root() {
    let stderr = run_err(&test_workspace(), &["self"]);
    // Should report an error about no package found at virtual workspace root
    assert!(
        stderr.contains("package") || stderr.contains("workspace"),
        "Expected error about no package at virtual root.\nStderr:\n{stderr}"
    );
}

// ===========================================================================
// C. `crate::module` syntax
// ===========================================================================

#[test]
fn crate_module_syntax() {
    let out = run_ok(&test_workspace(), &["core-lib::utils"]);
    assert!(
        out.contains("pub fn format_name"),
        "missing format_name in utils"
    );
    assert!(out.contains("pub enum LogLevel"), "missing LogLevel enum");
}

// ===========================================================================
// D. File path as module
// ===========================================================================

#[test]
fn file_path_from_package_dir() {
    let out = run_ok(&test_workspace().join("core-lib"), &["src/utils.rs"]);
    assert!(
        out.contains("pub fn format_name"),
        "missing format_name via file path"
    );
}

#[test]
fn self_with_file_path() {
    let out = run_ok(
        &test_workspace().join("core-lib"),
        &["self", "src/utils.rs"],
    );
    assert!(
        out.contains("pub fn format_name"),
        "missing format_name via self + file path"
    );
}

#[test]
#[ignore = "blocked: file path not resolved relative to package dir when cwd != package dir"]
fn pkg_with_file_path() {
    let out = run_ok(&test_workspace(), &["core-lib", "src/utils.rs"]);
    assert!(
        out.contains("pub fn format_name"),
        "missing format_name via pkg + file path"
    );
}

// ===========================================================================
// E. External crate (either — dependency, not workspace member)
// ===========================================================================

#[test]
fn external_crate_either() {
    let out = run_ok(&test_workspace(), &["either"]);
    assert!(
        out.contains("pub enum Either"),
        "missing Either enum from external crate"
    );
}

// ===========================================================================
// F. Visibility auto-detection (no explicit --at-package)
//
// These test that same_crate is automatically determined from cwd context.
// ===========================================================================

#[test]
fn auto_visibility_cross_crate() {
    // From app/, viewing core-lib → should hide pub(crate) items
    let out = run_ok(&test_workspace().join("app"), &["core-lib"]);
    assert!(
        !out.contains("InternalState"),
        "pub(crate) InternalState should be hidden in cross-crate view"
    );
    assert!(
        !out.contains("internal_helper"),
        "pub(crate) internal_helper should be hidden in cross-crate view"
    );
    // pub items should still be visible
    assert!(
        out.contains("pub struct Config"),
        "pub Config should be visible"
    );
}

#[test]
fn auto_visibility_same_crate() {
    // From core-lib/, viewing core-lib → should show pub(crate) items
    let out = run_ok(&test_workspace().join("core-lib"), &["core-lib"]);
    assert!(
        out.contains("InternalState"),
        "pub(crate) InternalState should be visible in same-crate view"
    );
    assert!(
        out.contains("internal_helper"),
        "pub(crate) internal_helper should be visible in same-crate view"
    );
}

#[test]
fn auto_visibility_reverse() {
    // From core-lib/, viewing app → should hide pub(crate) items of app
    let out = run_ok(&test_workspace().join("core-lib"), &["app"]);
    assert!(
        !out.contains("shutdown_internal"),
        "pub(crate) shutdown_internal should be hidden in cross-crate view"
    );
    assert!(out.contains("pub struct App"), "pub App should be visible");
}

// ===========================================================================
// G. Explicit --at-package override
// ===========================================================================

#[test]
fn at_package_cross_crate() {
    let out = run_ok(&test_workspace(), &["core-lib", "--at-package", "app"]);
    assert!(
        !out.contains("InternalState"),
        "pub(crate) InternalState should be hidden with --at-package app"
    );
    assert!(
        !out.contains("internal_helper"),
        "pub(crate) internal_helper should be hidden with --at-package app"
    );
}

#[test]
fn at_package_same_crate() {
    let out = run_ok(&test_workspace(), &["core-lib", "--at-package", "core-lib"]);
    assert!(
        out.contains("InternalState"),
        "pub(crate) InternalState should be visible with --at-package core-lib"
    );
    assert!(
        out.contains("internal_helper"),
        "pub(crate) internal_helper should be visible with --at-package core-lib"
    );
}

// ===========================================================================
// H. Depth and recursion
// ===========================================================================

#[test]
fn depth_zero() {
    let out = run_ok(&test_workspace(), &["core-lib", "--depth", "0"]);
    // Module should be collapsed — shown but contents not expanded
    assert!(
        out.contains("mod utils"),
        "module header should still appear at depth 0"
    );
    assert!(
        !out.contains("pub fn format_name"),
        "format_name should not appear at depth 0 (module collapsed)"
    );
}

#[test]
fn recursive() {
    let out = run_ok(&test_workspace(), &["core-lib", "--recursive"]);
    assert!(
        out.contains("pub fn format_name"),
        "format_name should appear with --recursive"
    );
}

// ===========================================================================
// I. Item filtering
// ===========================================================================

#[test]
fn no_structs() {
    let out = run_ok(&test_workspace(), &["core-lib", "--no-structs"]);
    assert!(
        !out.contains("struct Config"),
        "struct Config should be excluded by --no-structs"
    );
    assert!(
        out.contains("pub trait Processor"),
        "trait Processor should still be present"
    );
}

#[test]
fn no_functions() {
    let out = run_ok(&test_workspace(), &["core-lib", "--no-functions"]);
    assert!(
        !out.contains("fn create_default_config"),
        "create_default_config should be excluded by --no-functions"
    );
    assert!(
        out.contains("pub struct Config"),
        "struct Config should still be present"
    );
}

// ===========================================================================
// J. Error cases
// ===========================================================================

#[test]
fn nonexistent_crate() {
    let _stderr = run_err(&test_workspace(), &["nonexistent-crate"]);
}

#[test]
fn self_from_non_package() {
    // Same as self_from_virtual_root — virtual workspace root has no package
    let stderr = run_err(&test_workspace(), &["self"]);
    assert!(
        stderr.contains("package") || stderr.contains("workspace"),
        "Expected error about virtual workspace.\nStderr:\n{stderr}"
    );
}

// ===========================================================================
// K. Bare `cargo brief` (no TARGET — defaults to "self")
// ===========================================================================

#[test]
fn bare_cargo_brief_from_package_dir() {
    // Running `cargo brief` from a package dir should behave like `cargo brief self`
    let out = run_ok(&test_workspace().join("core-lib"), &[]);
    assert!(
        out.contains("pub struct Config"),
        "bare `cargo brief` from package dir should show Config struct"
    );
}

#[test]
fn bare_cargo_brief_from_virtual_root() {
    // Running `cargo brief` from virtual workspace root should fail (same as `cargo brief self`)
    let stderr = run_err(&test_workspace(), &[]);
    assert!(
        stderr.contains("package") || stderr.contains("workspace"),
        "Expected error about no package at virtual root.\nStderr:\n{stderr}"
    );
}
