//! Regression tests for facade crate rendering via `--crates`.
//!
//! Uses `hecs@0.11.0` as a pinned facade crate fixture. hecs defines types in
//! private modules and re-exports them at root via `pub use`.
//!
//! All tests require network access. Run with: `cargo test -- --ignored`

use cargo_brief::cli::BriefArgs;
use cargo_brief::run_pipeline;

fn hecs_args() -> BriefArgs {
    BriefArgs {
        crate_name: "self".to_string(),
        module_path: None,
        at_package: None,
        at_mod: None,
        depth: 1,
        recursive: true,
        all: false,
        no_docs: false,
        compact: false,
        no_structs: false,
        no_enums: false,
        no_traits: false,
        no_functions: false,
        no_aliases: false,
        no_constants: false,
        no_unions: false,
        no_macros: false,
        crates: Some("hecs@0.11.0".to_string()),
        expand_glob: false,
        search: None,
        search_limit: None,
        methods_of: None,
        features: None,
        no_cache: false,
        toolchain: "nightly".to_string(),
        manifest_path: None,
    }
}

#[test]
#[ignore = "network"]
fn hecs_private_modules_with_reachable_items() {
    let output = run_pipeline(&hecs_args()).unwrap();

    // Private modules are rendered because they contain reachable items
    assert!(
        output.contains("mod archetype {"),
        "archetype module should be rendered"
    );
    assert!(
        output.contains("mod entities {"),
        "entities module should be rendered"
    );
    assert!(
        output.contains("mod world {"),
        "world module should be rendered"
    );
}

#[test]
#[ignore = "network"]
fn hecs_reachable_types_inside_modules() {
    let output = run_pipeline(&hecs_args()).unwrap();

    assert!(
        output.contains("pub struct Archetype"),
        "Archetype struct inside module"
    );
    assert!(
        output.contains("pub struct World"),
        "World struct inside module"
    );
    assert!(
        output.contains("pub struct Entity"),
        "Entity struct inside module"
    );
}

#[test]
#[ignore = "network"]
fn hecs_no_pub_crate_items() {
    let output = run_pipeline(&hecs_args()).unwrap();

    assert!(
        !output.contains("pub(crate)"),
        "no pub(crate) items should appear in cross-crate view"
    );
}

#[test]
#[ignore = "network"]
fn hecs_pub_use_at_root() {
    let output = run_pipeline(&hecs_args()).unwrap();

    assert!(
        output.contains("pub use"),
        "pub use re-export lines should be present at root"
    );
}

#[test]
#[ignore = "network"]
fn hecs_nontrivial_output() {
    let output = run_pipeline(&hecs_args()).unwrap();

    let line_count = output.lines().count();
    assert!(
        line_count > 100,
        "output should be nontrivially long (> 100 lines), got {line_count}"
    );
}

#[test]
#[ignore = "network"]
fn hecs_search_no_pub_crate() {
    let mut args = hecs_args();
    args.search = Some("Archetype".to_string());
    let output = run_pipeline(&args).unwrap();

    assert!(output.contains("Archetype"), "search should find Archetype");
    assert!(
        !output.contains("pub(crate)"),
        "no pub(crate) items in search results"
    );
}
