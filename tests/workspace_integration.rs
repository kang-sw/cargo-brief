use cargo_brief::cli::BriefArgs;
use cargo_brief::run_pipeline;

fn workspace_args(crate_name: &str) -> BriefArgs {
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
        expand_glob: false,
        toolchain: "nightly".to_string(),
        manifest_path: Some("test_workspace/Cargo.toml".to_string()),
    }
}

// ============================================================
// Same-crate visibility
// ============================================================

#[test]
fn core_lib_same_crate_shows_pub_items() {
    let args = workspace_args("core-lib");
    let output = run_pipeline(&args).unwrap();

    assert!(output.contains("pub struct Config"), "Config visible");
    assert!(output.contains("pub name: String"), "pub field visible");
    assert!(
        output.contains("pub trait Processor"),
        "Processor trait visible"
    );
    assert!(
        output.contains("pub fn create_default_config()"),
        "pub function visible"
    );
    assert!(output.contains("pub const VERSION"), "pub constant visible");
}

#[test]
fn core_lib_same_crate_shows_pub_crate_items() {
    let mut args = workspace_args("core-lib");
    args.at_package = Some("core-lib".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub(crate)") && output.contains("internal_id"),
        "pub(crate) field visible in same crate: {output}"
    );
    assert!(
        output.contains("pub(crate)") && output.contains("InternalState"),
        "pub(crate) struct visible in same crate"
    );
    assert!(
        output.contains("pub(crate)") && output.contains("internal_helper"),
        "pub(crate) fn visible in same crate"
    );
    assert!(
        output.contains("pub(crate)") && output.contains("INTERNAL_LIMIT"),
        "pub(crate) const visible in same crate"
    );
}

// ============================================================
// External (cross-crate) visibility
// ============================================================

#[test]
fn core_lib_external_view_shows_pub_items() {
    let mut args = workspace_args("core-lib");
    args.at_package = Some("app".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub struct Config"),
        "Config visible externally"
    );
    assert!(
        output.contains("pub name: String"),
        "pub field visible externally"
    );
    assert!(
        output.contains("pub trait Processor"),
        "pub trait visible externally"
    );
    assert!(
        output.contains("pub fn create_default_config()"),
        "pub fn visible externally"
    );
    assert!(
        output.contains("pub const VERSION"),
        "pub const visible externally"
    );
}

#[test]
fn core_lib_external_view_hides_pub_crate_items() {
    let mut args = workspace_args("core-lib");
    args.at_package = Some("app".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(
        !output.contains("internal_id"),
        "pub(crate) field hidden externally"
    );
    assert!(
        !output.contains("InternalState"),
        "pub(crate) struct hidden externally"
    );
    assert!(
        !output.contains("internal_helper"),
        "pub(crate) fn hidden externally"
    );
    assert!(
        !output.contains("INTERNAL_LIMIT"),
        "pub(crate) const hidden externally"
    );
}

#[test]
fn core_lib_external_view_hides_crate_method() {
    let mut args = workspace_args("core-lib");
    args.at_package = Some("app".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(
        !output.contains("get_internal_id"),
        "pub(crate) method hidden externally"
    );
    assert!(
        output.contains("pub fn new("),
        "pub method visible externally"
    );
}

// ============================================================
// Struct field visibility across crate boundary
// ============================================================

#[test]
fn core_lib_external_view_struct_has_hidden_field_indicator() {
    let mut args = workspace_args("core-lib");
    args.at_package = Some("app".to_string());
    let output = run_pipeline(&args).unwrap();

    // Config has pub name, but internal_id (pub(crate)) and secret (private) are hidden
    // Should show { .. } or "private fields" indicator
    assert!(
        output.contains("pub name: String"),
        "pub field shown in external view"
    );
    assert!(
        !output.contains("secret"),
        "private field hidden externally"
    );
    // There should be some indication of hidden fields
    assert!(
        output.contains("private fields") || output.contains(".."),
        "hidden fields indicator present:\n{output}"
    );
}

// ============================================================
// Submodule visibility
// ============================================================

#[test]
fn core_lib_utils_same_crate_shows_all_visible() {
    let mut args = workspace_args("core-lib");
    args.module_path = Some("utils".to_string());
    args.at_package = Some("core-lib".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub fn format_name("),
        "pub fn visible in utils"
    );
    assert!(
        output.contains("pub enum LogLevel"),
        "pub enum visible in utils"
    );
    assert!(
        output.contains("crate_util"),
        "pub(crate) fn visible in same crate"
    );
}

#[test]
fn core_lib_utils_external_hides_crate_items() {
    let mut args = workspace_args("core-lib");
    args.module_path = Some("utils".to_string());
    args.at_package = Some("app".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub fn format_name("),
        "pub fn visible externally"
    );
    assert!(
        output.contains("pub enum LogLevel"),
        "pub enum visible externally"
    );
    assert!(
        !output.contains("crate_util"),
        "pub(crate) fn hidden externally"
    );
    assert!(
        !output.contains("UtilConfig"),
        "pub(crate) struct hidden externally"
    );
    assert!(
        !output.contains("parent_visible_helper"),
        "pub(super) fn hidden externally"
    );
}

// ============================================================
// Re-exports
// ============================================================

#[test]
fn core_lib_reexport_visible_at_root() {
    let args = workspace_args("core-lib");
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub use") && output.contains("format_name"),
        "re-export of format_name at crate root:\n{output}"
    );
}

// ============================================================
// App package
// ============================================================

#[test]
fn app_same_crate_view() {
    let mut args = workspace_args("app");
    args.at_package = Some("app".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(output.contains("pub struct App"), "App struct visible");
    assert!(output.contains("pub fn new()"), "pub method visible");
    assert!(output.contains("pub fn start("), "pub method visible");
    assert!(
        output.contains("shutdown_internal"),
        "pub(crate) method visible in same crate"
    );
    assert!(output.contains("pub fn run()"), "pub function visible");
}

#[test]
fn app_external_view() {
    let mut args = workspace_args("app");
    args.at_package = Some("core-lib".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("pub struct App"),
        "App struct visible externally"
    );
    assert!(
        output.contains("pub fn new()"),
        "pub method visible externally"
    );
    assert!(
        !output.contains("shutdown_internal"),
        "pub(crate) method hidden externally"
    );
}

// ============================================================
// Module path targeting
// ============================================================

#[test]
fn core_lib_target_utils_module_directly() {
    let mut args = workspace_args("core-lib");
    args.module_path = Some("utils".to_string());
    let output = run_pipeline(&args).unwrap();

    // Should show utils module contents
    assert!(
        output.contains("pub fn format_name("),
        "format_name in utils"
    );
    assert!(output.contains("pub enum LogLevel"), "LogLevel in utils");
    // Should NOT show root-level items
    assert!(
        !output.contains("pub struct Config"),
        "Config should not appear in utils module view"
    );
}

// ============================================================
// Depth control
// ============================================================

#[test]
fn core_lib_depth_zero_collapses_modules() {
    let mut args = workspace_args("core-lib");
    args.recursive = false;
    args.depth = 0;
    let output = run_pipeline(&args).unwrap();

    // utils module should be collapsed
    assert!(
        output.contains("mod utils { /* ... */ }"),
        "utils module collapsed at depth 0:\n{output}"
    );
    // Items inside utils should NOT appear
    assert!(
        !output.contains("pub fn format_name"),
        "format_name hidden at depth 0"
    );
    // Root-level items should still appear
    assert!(
        output.contains("pub struct Config") || output.contains("pub fn create_default_config"),
        "root items visible at depth 0:\n{output}"
    );
}

#[test]
fn core_lib_depth_one_shows_utils_contents() {
    let mut args = workspace_args("core-lib");
    args.recursive = false;
    args.depth = 1;
    let output = run_pipeline(&args).unwrap();

    // Root items visible
    assert!(output.contains("pub struct Config"), "Config at depth 1");
    // utils module contents should be expanded
    assert!(
        output.contains("pub fn format_name("),
        "format_name visible at depth 1"
    );
}

// ============================================================
// Crate header
// ============================================================

#[test]
fn core_lib_has_correct_crate_header() {
    let args = workspace_args("core-lib");
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.starts_with("// crate core_lib\n"),
        "core-lib crate header: got first line = {:?}",
        output.lines().next()
    );
}

#[test]
fn app_has_correct_crate_header() {
    let args = workspace_args("app");
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.starts_with("// crate app\n"),
        "app crate header: got first line = {:?}",
        output.lines().next()
    );
}

// ============================================================
// Versioned package specifier (pkg@version)
// ============================================================

#[test]
fn core_lib_versioned_specifier() {
    let args = workspace_args("core-lib@0.1.0");
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.starts_with("// crate core_lib\n"),
        "crate header with versioned specifier: got first line = {:?}",
        output.lines().next()
    );
    assert!(
        output.contains("pub struct Config"),
        "Config visible with versioned specifier"
    );
}

// ============================================================
// Trait impl rendering
// ============================================================

#[test]
fn core_lib_trait_impl_rendered() {
    let args = workspace_args("core-lib");
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("impl Processor for Config"),
        "trait impl rendered:\n{output}"
    );
}

// ============================================================
// Enum in submodule
// ============================================================

#[test]
fn core_lib_enum_in_utils() {
    let mut args = workspace_args("core-lib");
    args.module_path = Some("utils".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(output.contains("pub enum LogLevel"), "LogLevel enum");
    assert!(output.contains("Debug,"), "Debug variant");
    assert!(output.contains("Info,"), "Info variant");
    assert!(output.contains("Warn,"), "Warn variant");
    assert!(output.contains("Error,"), "Error variant");
}

// ============================================================
// Doc comments preserved
// ============================================================

#[test]
fn core_lib_doc_comments_preserved() {
    let args = workspace_args("core-lib");
    let output = run_pipeline(&args).unwrap();

    assert!(
        output.contains("/// Configuration for the system."),
        "struct doc comment"
    );
    assert!(
        output.contains("/// The public name field."),
        "field doc comment"
    );
    assert!(
        output.contains("/// A public trait for processing items."),
        "trait doc comment"
    );
}
