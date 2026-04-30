//! Regression tests for facade crate rendering via `-C`.
//!
//! Uses `hecs@0.11.0` as a pinned facade crate fixture. hecs defines types in
//! private modules and re-exports them at root via `pub use`.
//!
//! All tests require network access. Run with: `cargo test -- --ignored`

use cargo_brief::cli::{ApiArgs, FilterArgs, GlobalArgs, RemoteOpts, SearchArgs, TargetArgs};
use cargo_brief::{run_api_pipeline, run_search_pipeline};

fn hecs_args() -> (ApiArgs, RemoteOpts) {
    let args = ApiArgs {
        target: TargetArgs {
            crate_name: "hecs@0.11.0".to_string(),
            module_path: None,
            at_package: None,
            at_mod: None,
            manifest_path: None,
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
            no_feature_gates: false,
        },
        global: GlobalArgs {
            toolchain: "nightly".to_string(),
            verbose: false,
        },
        depth: 1,
        recursive: true,
        no_expand_glob: false,
    };
    let remote = RemoteOpts {
        crates: true,
        features: None,
        no_default_features: false,
        no_cache: false,
    };
    (args, remote)
}

#[test]
#[ignore = "network"]
fn hecs_private_modules_with_reachable_items() {
    let (args, remote) = hecs_args();
    let output = run_api_pipeline(&args, &remote).unwrap();

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
    let (args, remote) = hecs_args();
    let output = run_api_pipeline(&args, &remote).unwrap();

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
    let (args, remote) = hecs_args();
    let output = run_api_pipeline(&args, &remote).unwrap();

    assert!(
        !output.contains("pub(crate)"),
        "no pub(crate) items should appear in cross-crate view"
    );
}

#[test]
#[ignore = "network"]
fn hecs_pub_use_at_root() {
    let (args, remote) = hecs_args();
    let output = run_api_pipeline(&args, &remote).unwrap();

    assert!(
        output.contains("pub use"),
        "pub use re-export lines should be present at root"
    );
}

#[test]
#[ignore = "network"]
fn hecs_nontrivial_output() {
    let (args, remote) = hecs_args();
    let output = run_api_pipeline(&args, &remote).unwrap();

    let line_count = output.lines().count();
    assert!(
        line_count > 100,
        "output should be nontrivially long (> 100 lines), got {line_count}"
    );
}

#[test]
#[ignore = "network"]
fn hecs_search_no_pub_crate() {
    let args = SearchArgs {
        crate_name: "hecs@0.11.0".to_string(),
        patterns: vec!["Archetype".to_string()],
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
            no_feature_gates: false,
        },
        global: GlobalArgs {
            toolchain: "nightly".to_string(),
            verbose: false,
        },
        at_package: None,
        at_mod: None,
        manifest_path: None,
        limit: None,
        methods_of: None,
        search_kind: None,
        members: false,
        in_params: None,
        in_returns: None,
    };
    let remote = RemoteOpts {
        crates: true,
        features: None,
        no_default_features: false,
        no_cache: false,
    };
    let output = run_search_pipeline(&args, &remote).unwrap();

    assert!(output.contains("Archetype"), "search should find Archetype");
    assert!(
        !output.contains("pub(crate)"),
        "no pub(crate) items in search results"
    );
}
