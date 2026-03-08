use cargo_brief::cli::BriefArgs;
use cargo_brief::model::CrateModel;
use cargo_brief::render::render_module_api;
use cargo_brief::resolve;
use cargo_brief::rustdoc_json;

/// Generate the model from the test fixture once (per test).
fn fixture_model() -> CrateModel {
    let metadata = resolve::load_cargo_metadata(Some("test_fixture/Cargo.toml"))
        .expect("Failed to load cargo metadata");

    let json_path = rustdoc_json::generate_rustdoc_json(
        "test-fixture",
        "nightly",
        Some("test_fixture/Cargo.toml"),
        true,
        &metadata.target_dir,
    )
    .expect("Failed to generate rustdoc JSON for test fixture");

    let krate =
        rustdoc_json::parse_rustdoc_json(&json_path).expect("Failed to parse test fixture JSON");

    CrateModel::from_crate(krate)
}

fn default_args() -> BriefArgs {
    BriefArgs {
        crate_name: "test-fixture".to_string(),
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
        manifest_path: Some("test_fixture/Cargo.toml".to_string()),
    }
}

fn render_full(model: &CrateModel, args: &BriefArgs) -> String {
    render_module_api(model, args.module_path.as_deref(), args, None, true)
}

fn render_module(model: &CrateModel, args: &BriefArgs, module: &str) -> String {
    render_module_api(model, Some(module), args, None, true)
}

// === Struct Tests ===

#[test]
fn test_struct_fields_visible_same_crate() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub struct PubStruct"),
        "PubStruct should appear"
    );
    assert!(output.contains("pub pub_field: i32"), "pub field visible");
    assert!(
        output.contains("pub(crate) crate_field: i32"),
        "pub(crate) field visible in same crate"
    );
}

#[test]
fn test_struct_private_struct_hidden() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        !output.contains("PrivateStruct"),
        "PrivateStruct should be hidden"
    );
}

#[test]
fn test_struct_external_crate_view() {
    let model = fixture_model();
    let args = default_args();
    // Simulate external crate view
    let output = render_module_api(
        &model, None, &args, None, false, // same_crate = false
    );

    assert!(
        output.contains("pub struct PubStruct"),
        "PubStruct visible externally"
    );
    assert!(
        output.contains("pub pub_field: i32"),
        "pub field visible externally"
    );
    assert!(
        !output.contains("crate_field"),
        "pub(crate) field hidden externally"
    );
    assert!(
        !output.contains("CrateStruct"),
        "CrateStruct hidden externally"
    );
    assert!(
        !output.contains("SuperStruct"),
        "SuperStruct hidden externally"
    );
}

// === Enum Tests ===

#[test]
fn test_plain_enum() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub enum PlainEnum"),
        "PlainEnum should appear"
    );
    assert!(output.contains("Alpha,"), "Alpha variant");
    assert!(output.contains("Beta,"), "Beta variant");
    assert!(output.contains("Gamma,"), "Gamma variant");
}

#[test]
fn test_tuple_enum() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub enum TupleEnum"),
        "TupleEnum should appear"
    );
    assert!(output.contains("One(i32)"), "tuple variant with one field");
    assert!(
        output.contains("Two(String, bool)"),
        "tuple variant with two fields"
    );
    assert!(output.contains("Empty,"), "plain variant in tuple enum");
}

#[test]
fn test_struct_enum() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub enum StructEnum"),
        "StructEnum should appear"
    );
    assert!(output.contains("x: f64"), "struct variant field x");
    assert!(output.contains("y: f64"), "struct variant field y");
    assert!(output.contains("name: String"), "struct variant field name");
}

// === Function Tests ===

#[test]
fn test_free_functions() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub fn free_function(x: i32, y: i32) -> i32;"),
        "regular function"
    );
    assert!(
        output.contains("pub async fn async_function()"),
        "async function"
    );
    assert!(
        output.contains("pub const fn const_function(x: u32) -> u32;"),
        "const function"
    );
    assert!(
        output.contains("pub unsafe fn unsafe_function(ptr: *const u8) -> u8;"),
        "unsafe function"
    );
}

// === Generic Tests ===

#[test]
fn test_generic_struct() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub struct GenericStruct<T: Clone, U = ()>"),
        "generic struct with bounds and default"
    );
    assert!(output.contains("pub value: T"), "generic field T");
    assert!(output.contains("pub extra: U"), "generic field U");
}

#[test]
fn test_generic_trait() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub trait GenericTrait<T: Send + Sync>: Clone"),
        "generic trait with bounds"
    );
    assert!(output.contains("type Output;"), "associated type in trait");
    assert!(
        output.contains("fn process(&self, input: T)"),
        "generic method"
    );
}

#[test]
fn test_generic_function() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains(
            "pub fn generic_function<T: std::fmt::Debug + Clone>(items: &[T]) -> Vec<T>;"
        ),
        "generic function"
    );
}

// === Trait Tests ===

#[test]
fn test_trait_definition() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(output.contains("pub trait MyTrait"), "MyTrait definition");
    assert!(
        output.contains("fn do_thing(&self) -> bool;"),
        "trait method"
    );
}

#[test]
fn test_trait_impl() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("impl MyTrait for PubStruct"),
        "trait impl block"
    );
}

// === Constants and Statics ===

#[test]
fn test_constant() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(output.contains("pub const MY_CONST: i32"), "constant");
}

#[test]
fn test_static() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub static GLOBAL_COUNT:"),
        "static variable"
    );
    assert!(
        output.contains("pub static mut MUTABLE_GLOBAL: i32"),
        "mutable static"
    );
}

// === Macros ===

#[test]
fn test_macro() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("macro_rules! my_macro"),
        "macro_rules definition"
    );
}

// === Union ===

#[test]
fn test_union() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(output.contains("pub union MyUnion"), "union definition");
    assert!(output.contains("pub int_val: i32"), "union field int_val");
    assert!(
        output.contains("pub float_val: f32"),
        "union field float_val"
    );
}

// === Re-exports ===

#[test]
fn test_reexport() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub use outer::PubStruct as ReExported;"),
        "re-export with alias"
    );
}

// === Doc Comments ===

#[test]
fn test_doc_comments_preserved() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("/// A documented trait."),
        "trait doc comment"
    );
    assert!(output.contains("/// Trait method."), "method doc comment");
    assert!(
        output.contains("/// A plain enum (C-like)."),
        "enum doc comment"
    );
    assert!(
        output.contains("/// A regular public function."),
        "function doc comment"
    );
    assert!(
        output.contains("/// A generic struct."),
        "struct doc comment"
    );
    assert!(
        output.contains("/// A static variable."),
        "static doc comment"
    );
    assert!(output.contains("/// A union type."), "union doc comment");
}

// === Depth Control ===

#[test]
fn test_depth_zero_shows_collapsed_modules() {
    let model = fixture_model();
    let mut args = default_args();
    args.recursive = false;
    args.depth = 0;
    let output = render_full(&model, &args);

    // At depth 0, modules should be collapsed
    assert!(
        output.contains("mod outer { /* ... */ }"),
        "module collapsed at depth 0"
    );
    // Items inside outer should NOT appear (they're at depth 1)
    assert!(
        !output.contains("pub struct PubStruct"),
        "PubStruct hidden at depth 0"
    );
}

#[test]
fn test_depth_one_shows_outer_but_inner_collapsed() {
    let model = fixture_model();
    let mut args = default_args();
    args.recursive = false;
    args.depth = 1;
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub struct PubStruct"),
        "PubStruct at depth 1"
    );
    assert!(
        output.contains("mod inner { /* ... */ }"),
        "inner module collapsed at depth 1"
    );
}

// === Item Kind Filtering ===

#[test]
fn test_no_structs_flag() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_structs = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("pub struct PubStruct"),
        "structs filtered out"
    );
    assert!(output.contains("pub enum PlainEnum"), "enums still shown");
}

#[test]
fn test_no_enums_flag() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_enums = true;
    let output = render_full(&model, &args);

    assert!(!output.contains("pub enum PlainEnum"), "enums filtered out");
    assert!(
        output.contains("pub struct PubStruct"),
        "structs still shown"
    );
}

#[test]
fn test_no_functions_flag() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_functions = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("pub fn free_function"),
        "functions filtered out"
    );
    assert!(
        output.contains("pub struct PubStruct"),
        "structs still shown"
    );
}

#[test]
fn test_no_traits_flag() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_traits = true;
    let output = render_full(&model, &args);

    assert!(!output.contains("pub trait MyTrait"), "traits filtered out");
}

#[test]
fn test_no_constants_flag() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_constants = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("pub const MY_CONST"),
        "constants filtered out"
    );
    assert!(
        !output.contains("pub static GLOBAL_COUNT"),
        "statics also filtered by no_constants"
    );
}

#[test]
fn test_no_macros_flag() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_macros = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("macro_rules! my_macro"),
        "macros filtered out"
    );
}

#[test]
fn test_no_unions_flag() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_unions = true;
    let output = render_full(&model, &args);

    assert!(!output.contains("pub union MyUnion"), "unions filtered out");
}

// === Module Path ===

#[test]
fn test_target_module_outer() {
    let model = fixture_model();
    let args = default_args();
    let output = render_module(&model, &args, "outer");

    assert!(
        output.contains("pub struct PubStruct"),
        "PubStruct in outer module"
    );
    // Should not wrap in "mod outer" — we're rendering *contents* of outer
    assert!(
        !output.contains("pub use outer::PubStruct as ReExported"),
        "re-export is in root, not outer"
    );
}

#[test]
fn test_target_module_inner() {
    let model = fixture_model();
    let args = default_args();
    let output = render_module(&model, &args, "outer::inner");

    assert!(
        output.contains("pub struct InnerPub"),
        "InnerPub in inner module"
    );
    assert!(
        !output.contains("pub struct PubStruct"),
        "PubStruct not in inner"
    );
}

// === Visibility: Same Crate vs External ===

#[test]
fn test_same_crate_visibility() {
    let model = fixture_model();
    let args = default_args();
    let output = render_module_api(&model, None, &args, None, true);

    assert!(
        output.contains("pub(crate) struct CrateStruct"),
        "CrateStruct visible in same crate"
    );
}

#[test]
fn test_external_visibility_hides_crate_items() {
    let model = fixture_model();
    let args = default_args();
    let output = render_module_api(&model, None, &args, None, false);

    assert!(
        !output.contains("CrateStruct"),
        "CrateStruct hidden externally"
    );
    assert!(
        !output.contains("crate_method"),
        "crate_method hidden externally"
    );
    assert!(
        output.contains("pub fn pub_method"),
        "pub_method visible externally"
    );
}

// === Crate Header ===

#[test]
fn test_crate_header() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.starts_with("// crate test_fixture\n"),
        "crate header"
    );
}

// === Inherent Impl ===

#[test]
fn test_inherent_impl() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(output.contains("impl PubStruct {"), "inherent impl block");
    assert!(
        output.contains("pub fn pub_method(&self) -> i32;"),
        "method in impl block"
    );
}

// === Trait Impl Condensing ===

#[test]
fn test_trait_impl_is_one_liner() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    // Simple trait impl (no associated types) should be a one-liner with semicolon
    assert!(
        output.contains("impl MyTrait for PubStruct;"),
        "trait impl should be one-liner: got:\n{output}"
    );
    // Should NOT contain the expanded method body
    assert!(
        !output.contains("impl MyTrait for PubStruct {"),
        "trait impl should not have braces"
    );
}

#[test]
fn test_trait_impl_with_assoc_type_shows_type() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    // Trait impl with associated type should show the type but not methods
    assert!(
        output.contains("impl Converter for PubStruct {"),
        "trait impl with assoc type should have braces"
    );
    assert!(
        output.contains("type Output = String;"),
        "associated type should be shown"
    );
    // Methods should NOT be shown in condensed trait impl
    assert!(
        !output.contains("fn convert(&self) -> String;"),
        "methods should be omitted in trait impl with assoc type"
    );
}

// === Root Indent ===

#[test]
fn test_root_items_no_indent() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    // Lines after the crate header should start without 4-space indent
    let lines: Vec<&str> = output.lines().collect();
    // Find the "mod outer {" line — it should NOT be indented
    let mod_line = lines.iter().find(|l| l.contains("mod outer")).unwrap();
    assert!(
        mod_line.starts_with("mod outer"),
        "top-level module should have no indent, got: '{mod_line}'"
    );
}
