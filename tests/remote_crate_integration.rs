//! Integration tests for remote crate support via `--crates`.
//!
//! All tests are `#[ignore]` because they require network access and
//! download/compile crates from crates.io. Run with: `cargo test -- --ignored`

use cargo_brief::cli::{ApiArgs, FilterArgs, GlobalArgs, RemoteArgs, TargetArgs};
use cargo_brief::run_api_pipeline;

fn remote_args(spec: &str) -> ApiArgs {
    ApiArgs {
        target: TargetArgs {
            crate_name: "self".to_string(),
            module_path: None,
            at_package: None,
            at_mod: None,
            manifest_path: None,
        },
        remote: RemoteArgs {
            crates: Some(spec.to_string()),
            features: None,
            no_cache: false,
            clean: None,
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
        expand_glob: false,
    }
}

#[test]
#[ignore = "network: fetches from crates.io"]
fn remote_serde_latest() {
    let output = run_api_pipeline(&remote_args("serde")).expect("serde should resolve");
    assert!(
        output.contains("Serialize"),
        "expected Serialize in serde output\nActual output:\n{output}"
    );
}

#[test]
#[ignore = "network: fetches from crates.io"]
fn remote_serde_pinned() {
    let output =
        run_api_pipeline(&remote_args("serde@1.0.200")).expect("serde@1.0.200 should resolve");
    assert!(output.contains("serde"), "expected crate header for serde");
    assert!(
        output.contains("pub trait Serialize"),
        "expected Serialize trait"
    );
}

#[test]
#[ignore = "network: fetches from crates.io"]
fn remote_nonexistent() {
    let result = run_api_pipeline(&remote_args("this-crate-does-not-exist-xyzzy"));
    assert!(result.is_err(), "nonexistent crate should produce an error");
}

#[test]
#[ignore = "network: fetches from crates.io"]
fn remote_with_module_path() {
    let mut args = remote_args("either");
    args.target.module_path = Some("nonexistent".to_string());
    let output = run_api_pipeline(&args).expect("either with invalid module should still return");
    assert!(
        output.contains("module 'nonexistent' not found"),
        "expected module-not-found error\nActual output:\n{output}"
    );
}
