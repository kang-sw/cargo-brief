use cargo_brief::cli::BriefArgs;
use cargo_brief::model::CrateModel;
use cargo_brief::render::render_module_api;
use cargo_brief::resolve;
use cargo_brief::rustdoc_json;
use cargo_brief::search;

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
        no_docs: false,
        no_crate_docs: false,
        doc_lines: None,
        compact: false,
        verbose_metadata: false,
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
        search: None,
        search_limit: None,
        methods_of: None,
        features: None,
        no_cache: false,
        clean: None,
        toolchain: "nightly".to_string(),
        manifest_path: Some("test_fixture/Cargo.toml".to_string()),
    }
}

fn render_full(model: &CrateModel, args: &BriefArgs) -> String {
    render_module_api(model, args.module_path.as_deref(), args, None, true, None)
}

fn render_module(model: &CrateModel, args: &BriefArgs, module: &str) -> String {
    render_module_api(model, Some(module), args, None, true, None)
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
        &model, None, &args, None, false, None, // same_crate = false
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

    // Simple trait impls (no assoc items) are collapsed into summary comment
    assert!(
        output.contains("MyTrait"),
        "MyTrait should appear in summary comment:\n{output}"
    );
    // Rich trait impl (Converter has assoc type) should still be expanded
    assert!(
        output.contains("impl Converter for PubStruct"),
        "rich trait impl should remain expanded:\n{output}"
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
        output.contains("pub use outer::PubStruct as ReExported; // struct"),
        "re-export with alias and kind annotation"
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
    let output = render_module_api(&model, None, &args, None, true, None);

    assert!(
        output.contains("pub(crate) struct CrateStruct"),
        "CrateStruct visible in same crate"
    );
}

#[test]
fn test_external_visibility_hides_crate_items() {
    let model = fixture_model();
    let args = default_args();
    let output = render_module_api(&model, None, &args, None, false, None);

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
fn test_simple_trait_impl_collapsed_to_summary() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    // Simple trait impls (no associated types/constants) should be collapsed into
    // a summary comment, not rendered as individual `impl ... { .. }` lines
    assert!(
        !output.contains("impl MyTrait for PubStruct { .. }"),
        "simple trait impl should NOT appear as individual line:\n{output}"
    );
    // Should appear in summary comment instead
    let summary_line = output
        .lines()
        .find(|l| l.contains("// PubStruct:") && l.contains("MyTrait"));
    assert!(
        summary_line.is_some(),
        "PubStruct summary should mention MyTrait:\n{output}"
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

// === Search Mode Tests ===

fn search_output(model: &CrateModel, pattern: &str) -> String {
    let args = default_args();
    search::render_search(model, pattern, &args, None, true, None)
}

#[test]
fn search_finds_struct_by_name() {
    let model = fixture_model();
    let output = search_output(&model, "PubStruct");
    assert!(
        output.contains("struct outer::PubStruct"),
        "Should find PubStruct:\n{output}"
    );
}

#[test]
fn search_finds_struct_field() {
    let model = fixture_model();
    let output = search_output(&model, "pub_field");
    assert!(
        output.contains("field outer::PubStruct::pub_field: i32"),
        "Should find pub_field:\n{output}"
    );
}

#[test]
fn search_finds_enum_variant() {
    let model = fixture_model();
    let output = search_output(&model, "Alpha");
    assert!(
        output.contains("variant outer::PlainEnum::Alpha"),
        "Should find Alpha variant:\n{output}"
    );
}

#[test]
fn search_finds_tuple_variant() {
    let model = fixture_model();
    let output = search_output(&model, "TupleEnum One");
    let output_lower = output.to_lowercase();
    assert!(
        output_lower.contains("variant outer::tupleenum::one"),
        "Should find One variant with multi-word AND:\n{output}"
    );
}

#[test]
fn search_finds_struct_variant() {
    let model = fixture_model();
    let output = search_output(&model, "Point");
    assert!(
        output.contains("variant outer::StructEnum::Point"),
        "Should find Point variant:\n{output}"
    );
    assert!(
        output.contains("x: f64"),
        "Point variant should show fields:\n{output}"
    );
}

#[test]
fn search_finds_method() {
    let model = fixture_model();
    let output = search_output(&model, "pub_method");
    assert!(
        output.contains("fn outer::PubStruct::pub_method"),
        "Should find pub_method:\n{output}"
    );
}

#[test]
fn search_finds_free_function() {
    let model = fixture_model();
    let output = search_output(&model, "free_function");
    assert!(
        output.contains("fn outer::free_function"),
        "Should find free_function:\n{output}"
    );
}

#[test]
fn search_finds_trait() {
    let model = fixture_model();
    let output = search_output(&model, "MyTrait");
    assert!(
        output.contains("trait outer::MyTrait"),
        "Should find MyTrait:\n{output}"
    );
}

#[test]
fn search_finds_trait_method() {
    let model = fixture_model();
    let output = search_output(&model, "do_thing");
    assert!(
        output.contains("fn outer::MyTrait::do_thing"),
        "Should find trait method do_thing:\n{output}"
    );
}

#[test]
fn search_finds_constant() {
    let model = fixture_model();
    let output = search_output(&model, "MY_CONST");
    assert!(
        output.contains("const outer::MY_CONST"),
        "Should find MY_CONST:\n{output}"
    );
}

#[test]
fn search_finds_static() {
    let model = fixture_model();
    let output = search_output(&model, "GLOBAL_COUNT");
    assert!(
        output.contains("static outer::GLOBAL_COUNT"),
        "Should find GLOBAL_COUNT:\n{output}"
    );
}

#[test]
fn search_finds_type_alias() {
    let model = fixture_model();
    let output = search_output(&model, "Alias");
    assert!(
        output.contains("type outer::Alias"),
        "Should find type alias:\n{output}"
    );
}

#[test]
fn search_finds_union() {
    let model = fixture_model();
    let output = search_output(&model, "MyUnion");
    assert!(
        output.contains("union outer::MyUnion"),
        "Should find MyUnion:\n{output}"
    );
}

#[test]
fn search_finds_union_field() {
    let model = fixture_model();
    let output = search_output(&model, "int_val");
    assert!(
        output.contains("field outer::MyUnion::int_val"),
        "Should find union field:\n{output}"
    );
}

#[test]
fn search_finds_macro() {
    let model = fixture_model();
    let output = search_output(&model, "my_macro");
    assert!(
        output.contains("macro my_macro!"),
        "Should find macro:\n{output}"
    );
}

#[test]
fn search_case_insensitive() {
    let model = fixture_model();
    let output = search_output(&model, "pubstruct");
    assert!(
        output.contains("struct outer::PubStruct"),
        "Case-insensitive search should find PubStruct:\n{output}"
    );
}

#[test]
fn search_multi_word_and() {
    let model = fixture_model();
    let output = search_output(&model, "outer method");
    assert!(
        output.contains("fn outer::PubStruct::pub_method"),
        "Multi-word AND should match:\n{output}"
    );
    // Should not contain items that only match one word
    assert!(
        !output.contains("free_function"),
        "free_function doesn't match 'method':\n{output}"
    );
}

#[test]
fn search_no_functions_excludes_methods() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_functions = true;
    let output = search::render_search(&model, "pub_method", &args, None, true, None);
    assert!(
        !output.contains("fn "),
        "--no-functions should exclude methods:\n{output}"
    );
}

#[test]
fn search_result_count_in_header() {
    let model = fixture_model();
    let output = search_output(&model, "Alpha");
    assert!(
        output.contains("(1 results)"),
        "Header should show result count:\n{output}"
    );
}

#[test]
fn search_shows_doc_comment() {
    let model = fixture_model();
    let output = search_output(&model, "free_function");
    assert!(
        output.contains("/// A regular public function."),
        "Should show first line of doc comment:\n{output}"
    );
}

#[test]
fn search_associated_type() {
    let model = fixture_model();
    let output = search_output(&model, "Converter Output");
    assert!(
        output.contains("type outer::Converter::Output"),
        "Should find trait associated type:\n{output}"
    );
}

// === Search Mode Phase 2: Sorting, Re-exports, Limit ===

#[test]
fn search_results_sorted_by_kind_then_alpha() {
    let model = fixture_model();
    // Search for something that matches multiple kinds
    let output = search_output(&model, "outer");
    let lines: Vec<&str> = output
        .lines()
        .filter(|l| !l.starts_with("//") && !l.starts_with("///"))
        .collect();
    // Verify functions come before structs, structs before enums, etc.
    let first_fn = lines.iter().position(|l| l.starts_with("fn "));
    let first_struct = lines.iter().position(|l| l.starts_with("struct "));
    let first_enum = lines.iter().position(|l| l.starts_with("enum "));
    let first_field = lines.iter().position(|l| l.starts_with("field "));
    let first_variant = lines.iter().position(|l| l.starts_with("variant "));
    // fn < struct < enum < field < variant
    if let (Some(f), Some(s)) = (first_fn, first_struct) {
        assert!(f < s, "functions should come before structs:\n{output}");
    }
    if let (Some(s), Some(e)) = (first_struct, first_enum) {
        assert!(s < e, "structs should come before enums:\n{output}");
    }
    if let (Some(e), Some(f)) = (first_enum, first_field) {
        assert!(e < f, "enums should come before fields:\n{output}");
    }
    if let (Some(f), Some(v)) = (first_field, first_variant) {
        assert!(f < v, "fields should come before variants:\n{output}");
    }
}

#[test]
fn search_results_alpha_within_kind() {
    let model = fixture_model();
    let output = search_output(&model, "Enum");
    let enum_lines: Vec<&str> = output.lines().filter(|l| l.starts_with("enum ")).collect();
    if enum_lines.len() >= 2 {
        // Verify alphabetical ordering within enums
        for pair in enum_lines.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "enums should be alphabetically sorted: {:?} vs {:?}",
                pair[0],
                pair[1]
            );
        }
    }
}

#[test]
fn search_finds_reexport() {
    let model = fixture_model();
    let output = search_output(&model, "ReExported");
    assert!(
        output.contains("use outer::PubStruct as ReExported"),
        "Should find re-export with alias:\n{output}"
    );
}

#[test]
fn search_limit_truncates() {
    let model = fixture_model();
    let mut args = default_args();
    args.search_limit = Some("3".to_string());
    let output = search::render_search(&model, "outer", &args, None, true, None);
    // Count non-comment, non-doc lines (actual results)
    let result_lines: Vec<&str> = output
        .lines()
        .filter(|l| !l.starts_with("//") && !l.starts_with("///"))
        .collect();
    assert!(
        result_lines.len() <= 3,
        "Should display at most 3 results, got {}:\n{output}",
        result_lines.len()
    );
    assert!(
        output.contains("// ... and "),
        "Should show truncation message:\n{output}"
    );
    assert!(
        output.contains(" more results"),
        "Should show 'more results' suffix:\n{output}"
    );
}

#[test]
fn search_limit_none_shows_all() {
    let model = fixture_model();
    let args = default_args();
    let output = search::render_search(&model, "outer", &args, None, true, None);
    assert!(
        !output.contains("// ... and "),
        "No truncation without limit:\n{output}"
    );
}

#[test]
fn search_limit_paging() {
    let model = fixture_model();
    let mut args = default_args();
    // First get total to validate paging
    let full_output = search::render_search(&model, "outer", &args, None, true, None);
    let full_lines: Vec<&str> = full_output
        .lines()
        .filter(|l| !l.starts_with("//") && !l.starts_with("///"))
        .collect();
    let total = full_lines.len();
    assert!(total > 5, "Need enough results to test paging, got {total}");

    // Page: skip 2, show 3
    args.search_limit = Some("2:3".to_string());
    let output = search::render_search(&model, "outer", &args, None, true, None);

    let result_lines: Vec<&str> = output
        .lines()
        .filter(|l| !l.starts_with("//") && !l.starts_with("///"))
        .collect();
    assert_eq!(
        result_lines.len(),
        3,
        "Should display exactly 3 results:\n{output}"
    );
    assert!(
        output.contains("// (skipped 2 results)"),
        "Should show skipped message:\n{output}"
    );
    assert!(
        output.contains("// ... and "),
        "Should show trailing truncation:\n{output}"
    );
    // The 3 displayed should match positions 2..5 of the full sorted list
    for (i, line) in result_lines.iter().enumerate() {
        assert_eq!(
            *line,
            full_lines[2 + i],
            "Paged result {i} should match full result at index {}",
            2 + i
        );
    }
}

// === --no-docs Tests ===

#[test]
fn no_docs_suppresses_doc_comments() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_docs = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("///"),
        "--no-docs should suppress all doc comments:\n{output}"
    );
    // Items themselves should still be present
    assert!(
        output.contains("pub struct PubStruct"),
        "PubStruct should still appear with --no-docs"
    );
}

#[test]
fn no_docs_search_suppresses_doc_comments() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_docs = true;
    let output = search::render_search(&model, "free_function", &args, None, true, None);

    assert!(
        !output.contains("///"),
        "--no-docs should suppress doc comments in search:\n{output}"
    );
    assert!(
        output.contains("fn outer::free_function"),
        "function should still appear:\n{output}"
    );
}

// === --doc-lines Tests ===

#[test]
fn test_doc_lines_limits_output() {
    let model = fixture_model();
    let mut args = default_args();
    args.doc_lines = Some(1);
    let output = render_full(&model, &args);

    // Should contain first line of doc comments
    assert!(
        output.contains("///"),
        "--doc-lines 1 should show first doc line:\n{output}"
    );
    // Multi-line doc comments should be truncated to 1 line
    // The generic struct has "A generic struct." as first line — check it's there
    assert!(
        output.contains("/// A generic struct."),
        "first doc line should appear:\n{output}"
    );
}

#[test]
fn test_doc_lines_zero_suppresses_docs() {
    let model = fixture_model();
    let mut args = default_args();
    args.doc_lines = Some(0);
    let output = render_full(&model, &args);

    assert!(
        !output.contains("///"),
        "--doc-lines 0 should suppress all doc comments:\n{output}"
    );
    assert!(
        output.contains("pub struct PubStruct"),
        "items should still appear:\n{output}"
    );
}

// === --compact Tests ===

#[test]
fn compact_struct_collapsed() {
    let model = fixture_model();
    let mut args = default_args();
    args.compact = true;
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub struct PubStruct { .. }"),
        "compact should collapse struct fields:\n{output}"
    );
    assert!(
        !output.contains("pub_field: i32"),
        "compact should hide field details:\n{output}"
    );
}

#[test]
fn compact_enum_variants_name_only() {
    let model = fixture_model();
    let mut args = default_args();
    args.compact = true;
    let output = render_full(&model, &args);

    // PlainEnum should have all variants on one line
    assert!(
        output.contains("Alpha, Beta, Gamma"),
        "compact enum should show variant names:\n{output}"
    );
}

#[test]
fn compact_trait_collapsed() {
    let model = fixture_model();
    let mut args = default_args();
    args.compact = true;
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub trait MyTrait { .. }"),
        "compact should collapse trait:\n{output}"
    );
    assert!(
        !output.contains("fn do_thing"),
        "compact should hide trait methods:\n{output}"
    );
}

#[test]
fn compact_impl_collapsed() {
    let model = fixture_model();
    let mut args = default_args();
    args.compact = true;
    let output = render_full(&model, &args);

    assert!(
        output.contains("impl PubStruct { .. }"),
        "compact should collapse inherent impl:\n{output}"
    );
    assert!(
        !output.contains("pub fn pub_method"),
        "compact should hide impl methods:\n{output}"
    );
}

#[test]
fn compact_suppresses_docs() {
    let model = fixture_model();
    let mut args = default_args();
    args.compact = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("///"),
        "compact implies no_docs:\n{output}"
    );
}

// === Search Impl Summary Tests ===

#[test]
fn search_struct_shows_impl_summary() {
    let model = fixture_model();
    let args = default_args();
    let output = search::render_search(&model, "PubStruct", &args, None, true, None);

    assert!(
        output.contains("// impl"),
        "search should show impl summary for struct:\n{output}"
    );
}

// === --methods-of Tests ===

#[test]
fn methods_of_shows_only_methods_and_fields() {
    let model = fixture_model();
    let mut args = default_args();
    args.methods_of = Some("PubStruct".to_string());
    // Simulate what run_pipeline does: translate methods_of into search + exclusion flags
    args.search = Some("PubStruct".to_string());
    args.no_structs = true;
    args.no_enums = true;
    args.no_traits = true;
    args.no_unions = true;
    args.no_constants = true;
    args.no_macros = true;
    args.no_aliases = true;
    let output = search::render_search(&model, "PubStruct", &args, None, true, None);

    // Should contain methods
    assert!(
        output.contains("fn outer::PubStruct::pub_method"),
        "should show methods:\n{output}"
    );
    // Should contain fields
    assert!(
        output.contains("field outer::PubStruct::pub_field"),
        "should show fields:\n{output}"
    );
    // Should NOT contain the struct definition itself
    assert!(
        !output.contains("struct outer::PubStruct"),
        "should not show struct definition:\n{output}"
    );
    // Should NOT contain enums or traits
    assert!(
        !output.contains("enum "),
        "should not show enums:\n{output}"
    );
    assert!(
        !output.contains("trait "),
        "should not show traits:\n{output}"
    );
}

// === Where Clause Tests ===

#[test]
fn test_where_fn() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("where\n"),
        "where_fn should have a where clause:\n{output}"
    );
    assert!(
        output.contains("T: std::fmt::Display + Clone"),
        "where_fn should show T bound:\n{output}"
    );
    assert!(
        output.contains("U: Into<String>"),
        "where_fn should show U bound:\n{output}"
    );
}

#[test]
fn test_multi_where() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("T: std::fmt::Display"),
        "multi_where should show T bound:\n{output}"
    );
    assert!(
        output.contains("U: std::fmt::Debug + Clone"),
        "multi_where should show U bound:\n{output}"
    );
    assert!(
        output.contains("V: Into<String> + Send"),
        "multi_where should show V bound:\n{output}"
    );
}

#[test]
fn test_lifetime_where() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("'b: 'a"),
        "lifetime_where should show lifetime bound:\n{output}"
    );
}

#[test]
fn test_where_struct() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub struct WhereStruct<T> where T: std::fmt::Debug"),
        "WhereStruct should have where clause:\n{output}"
    );
}

#[test]
fn test_where_trait() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("pub trait WhereTrait<T> where T: Clone"),
        "WhereTrait should have where clause:\n{output}"
    );
}

#[test]
fn test_where_type_alias() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("type WhereAlias<T> where T: Ord"),
        "WhereAlias should have where clause:\n{output}"
    );
}

#[test]
fn test_where_impl_block() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("impl<T> WhereStruct<T> where T: std::fmt::Debug + Clone"),
        "impl block for WhereStruct should have where clause:\n{output}"
    );
}

#[test]
fn test_where_search_mode() {
    let model = fixture_model();
    let args = default_args();
    let output = search::render_search(&model, "where_fn", &args, None, true, None);

    assert!(
        output.contains("where T: std::fmt::Display + Clone, U: Into<String>"),
        "search mode should show compact where clause:\n{output}"
    );
}

// === Rendering Bug Fixes ===

#[test]
fn test_impl_trait_resugared() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    // impl Trait should be re-sugared, not shown as synthetic generic
    assert!(
        output.contains("impl_trait_fn(val: impl"),
        "impl Trait should be re-sugared in parameter:\n{output}"
    );
    // Should NOT have synthetic generic param in angle brackets
    assert!(
        !output.contains("impl_trait_fn<impl"),
        "synthetic generic should not appear in generics:\n{output}"
    );
}

#[test]
fn test_multi_impl_trait_resugared() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("a: impl") && output.contains("b: impl"),
        "multiple impl Trait params should be re-sugared:\n{output}"
    );
}

#[test]
fn test_impl_trait_search_mode() {
    let model = fixture_model();
    let args = default_args();
    let output = search::render_search(&model, "impl_trait_fn", &args, None, true, None);

    assert!(
        output.contains("val: impl"),
        "search mode should show re-sugared impl Trait:\n{output}"
    );
    assert!(
        !output.contains("<impl"),
        "search mode should not show synthetic generics:\n{output}"
    );
}

#[test]
fn test_qualified_path_no_empty_trait() {
    // This tests that <T as >::Output doesn't appear — format_type fallback works.
    // The test fixture Converter trait has associated type Output, used in:
    // fn convert(&self) -> <Self as Converter>::Output
    // which rustdoc may render with an empty trait path.
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    // Should not contain "<X as >::" pattern (empty trait path)
    assert!(
        !output.contains("as >::"),
        "Should not have empty trait path in qualified types:\n{output}"
    );
}

// === Attribute Rendering Tests ===

#[test]
fn test_deprecated_function_attr() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("#[deprecated(since = \"0.1.0\", note = \"use new_function instead\")]"),
        "deprecated function should show full #[deprecated(...)]:\n{output}"
    );
}

#[test]
fn test_deprecated_struct_attr() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("#[deprecated = \"old struct\"]"),
        "deprecated struct should show #[deprecated = \"...\"]:\n{output}"
    );
}

#[test]
fn test_non_exhaustive_enum_attr() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("#[non_exhaustive]"),
        "non-exhaustive enum should show #[non_exhaustive]:\n{output}"
    );
}

#[test]
fn test_verbose_metadata_shows_repr() {
    let model = fixture_model();
    let mut args = default_args();
    args.verbose_metadata = true;
    let output = render_full(&model, &args);

    assert!(
        output.contains("#[repr(C)]"),
        "--verbose-metadata should show #[repr(C)] on MyUnion:\n{output}"
    );
}

#[test]
fn test_default_hides_repr() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        !output.contains("#[repr(C)]"),
        "default mode should NOT show #[repr(C)]:\n{output}"
    );
}

#[test]
fn test_search_deprecated_marker() {
    let model = fixture_model();
    let args = default_args();
    let output = search::render_search(&model, "deprecated_function", &args, None, true, None);

    assert!(
        output.contains("[deprecated]"),
        "search should show [deprecated] marker:\n{output}"
    );
}

#[test]
fn test_search_non_exhaustive_marker() {
    let model = fixture_model();
    let args = default_args();
    let output = search::render_search(&model, "NonExhaustiveEnum", &args, None, true, None);

    assert!(
        output.contains("[non_exhaustive]"),
        "search should show [non_exhaustive] marker:\n{output}"
    );
}

// === Crate-Level Doc Tests ===

#[test]
fn test_crate_docs_rendered() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    assert!(
        output.contains("//! Test fixture crate for cargo-brief integration tests."),
        "crate-level docs should appear in output:\n{output}"
    );
    assert!(
        output.contains("//!\n"),
        "empty doc line should render as bare //!:\n{output}"
    );
    assert!(
        output.contains("//! This crate exercises all supported item types."),
        "second paragraph should appear:\n{output}"
    );
}

#[test]
fn test_crate_docs_after_header() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    let header_pos = output
        .find("// crate test_fixture")
        .expect("crate header missing");
    let doc_pos = output
        .find("//! Test fixture crate")
        .expect("crate doc missing");
    assert!(
        doc_pos > header_pos,
        "crate docs should appear after the crate header"
    );
}

#[test]
fn test_crate_docs_suppressed_by_no_docs() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_docs = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("//!"),
        "--no-docs should suppress crate-level docs:\n{output}"
    );
}

#[test]
fn test_crate_docs_suppressed_by_compact() {
    let model = fixture_model();
    let mut args = default_args();
    args.compact = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("//!"),
        "--compact should suppress crate-level docs:\n{output}"
    );
}

#[test]
fn test_crate_docs_limited_by_doc_lines() {
    let model = fixture_model();
    let mut args = default_args();
    args.doc_lines = Some(1);
    let output = render_full(&model, &args);

    assert!(
        output.contains("//! Test fixture crate"),
        "--doc-lines 1 should show first line:\n{output}"
    );
    // The second content line should NOT appear (line 0 = first line, line 1 = empty, already over limit)
    let crate_doc_lines: Vec<&str> = output.lines().filter(|l| l.starts_with("//!")).collect();
    assert_eq!(
        crate_doc_lines.len(),
        1,
        "--doc-lines 1 should limit to 1 crate doc line, got: {crate_doc_lines:?}"
    );
}

#[test]
fn test_crate_docs_suppressed_by_doc_lines_zero() {
    let model = fixture_model();
    let mut args = default_args();
    args.doc_lines = Some(0);
    let output = render_full(&model, &args);

    assert!(
        !output.contains("//!"),
        "--doc-lines 0 should suppress crate-level docs:\n{output}"
    );
}

#[test]
fn test_crate_docs_suppressed_by_no_crate_docs() {
    let model = fixture_model();
    let mut args = default_args();
    args.no_crate_docs = true;
    let output = render_full(&model, &args);

    assert!(
        !output.contains("//!"),
        "--no-crate-docs should suppress crate-level docs:\n{output}"
    );
    // Item docs should still be present
    assert!(
        output.contains("///"),
        "--no-crate-docs should NOT suppress item docs:\n{output}"
    );
}

#[test]
fn test_crate_docs_not_in_search_mode() {
    let model = fixture_model();
    let args = default_args();
    let output = search::render_search(&model, "PubStruct", &args, None, true, None);

    assert!(
        !output.contains("//!"),
        "search mode should not show crate-level docs:\n{output}"
    );
}

// === Trait Impl Collapsing Tests ===

#[test]
fn test_trait_impl_summary_format() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    // DerivedStruct should have a summary comment with derived traits
    let summary = output.lines().find(|l| l.contains("// DerivedStruct:"));
    assert!(
        summary.is_some(),
        "DerivedStruct should have a trait impl summary:\n{output}"
    );
    let summary = summary.unwrap();
    assert!(
        summary.contains("Clone"),
        "summary should include Clone: {summary}"
    );
    assert!(
        summary.contains("Debug"),
        "summary should include Debug: {summary}"
    );
    assert!(
        summary.contains("Eq"),
        "summary should include Eq: {summary}"
    );
    assert!(
        summary.contains("Hash"),
        "summary should include Hash: {summary}"
    );
    assert!(
        summary.contains("PartialEq"),
        "summary should include PartialEq: {summary}"
    );
    assert!(
        summary.contains("Display"),
        "summary should include Display: {summary}"
    );
}

#[test]
fn test_trait_impl_summary_sorted() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    let summary = output
        .lines()
        .find(|l| l.contains("// DerivedStruct:"))
        .expect("DerivedStruct summary missing");

    // Extract trait names from the summary comment
    let traits_part = summary.split(": ").nth(1).unwrap();
    let traits: Vec<&str> = traits_part.split(", ").collect();
    let mut sorted = traits.clone();
    sorted.sort();
    assert_eq!(traits, sorted, "traits should be alphabetically sorted");
}

#[test]
fn test_trait_impl_all_expands() {
    let model = fixture_model();
    let mut args = default_args();
    args.all = true;
    let output = render_full(&model, &args);

    // With --all, simple trait impls should be rendered individually, not collapsed
    assert!(
        output.contains("impl MyTrait for PubStruct { .. }"),
        "--all should expand simple trait impls:\n{output}"
    );
}

#[test]
fn test_trait_impl_rich_not_collapsed() {
    let model = fixture_model();
    let args = default_args();
    let output = render_full(&model, &args);

    // Converter has associated type Output — should remain expanded
    assert!(
        output.contains("impl Converter for PubStruct {"),
        "rich trait impl should remain expanded:\n{output}"
    );
    assert!(
        output.contains("type Output = String;"),
        "associated type should be shown:\n{output}"
    );
}
