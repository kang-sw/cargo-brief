//! Integration tests for version-normalized cache directories and crates.io version resolution.
//!
//! All tests are `#[ignore]` because they require network access to crates.io.
//! Run with: `cargo test --test version_cache_integration -- --ignored`

use cargo_brief::cli::{ApiArgs, FilterArgs, GlobalArgs, RemoteArgs, TargetArgs};
use cargo_brief::remote::{clean_cache, fetch_resolved_version, resolve_workspace};
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

/// Save and restore CARGO_BRIEF_CACHE_DIR around a closure that receives the temp path.
fn with_temp_cache<F: FnOnce(&std::path::Path)>(f: F) {
    let test_dir = tempfile::tempdir().unwrap();
    let original = std::env::var("CARGO_BRIEF_CACHE_DIR").ok();
    // SAFETY: test-only env manipulation, network tests run with --ignored (typically serial)
    unsafe { std::env::set_var("CARGO_BRIEF_CACHE_DIR", test_dir.path()) };
    f(test_dir.path());
    match original {
        Some(v) => unsafe { std::env::set_var("CARGO_BRIEF_CACHE_DIR", v) },
        None => unsafe { std::env::remove_var("CARGO_BRIEF_CACHE_DIR") },
    }
}

#[test]
#[ignore = "network: requires crates.io API access"]
fn fetch_resolved_version_bare_spec() {
    let ver = fetch_resolved_version("serde", "*").unwrap();
    // Should be a valid semver: x.y.z
    let parts: Vec<&str> = ver.split('.').collect();
    assert_eq!(parts.len(), 3, "expected semver x.y.z, got '{ver}'");
    assert!(
        parts.iter().all(|p| p.parse::<u64>().is_ok()),
        "expected numeric semver parts, got '{ver}'"
    );
}

#[test]
#[ignore = "network: requires crates.io API access"]
fn fetch_resolved_version_major_range() {
    let ver = fetch_resolved_version("serde", "1").unwrap();
    assert!(ver.starts_with("1."), "expected version 1.x.y, got '{ver}'");
}

#[test]
#[ignore = "network: requires crates.io API access"]
fn resolve_workspace_creates_normalized_dir() {
    with_temp_cache(|cache_path| {
        let (ws, resolved) = resolve_workspace("serde@1.0.200", None, false).unwrap();
        assert_eq!(resolved, Some("1.0.200".to_string()));

        let dir_name = ws.path().file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(dir_name, "serde[1.0.200]");

        // Cargo.toml should contain exact pin
        let toml = std::fs::read_to_string(ws.path().join("Cargo.toml")).unwrap();
        assert!(
            toml.contains("=1.0.200"),
            "Cargo.toml should contain =1.0.200, got:\n{toml}"
        );

        // Dir should be under the cache path
        assert!(ws.path().starts_with(cache_path));
    });
}

#[test]
#[ignore = "network: requires crates.io API access"]
fn resolve_workspace_same_version_reuses_dir() {
    with_temp_cache(|_cache_path| {
        // First call with bare spec
        let (ws1, ver1) = resolve_workspace("serde", None, false).unwrap();
        let ver1 = ver1.unwrap();

        // Second call with major-pinned spec — should resolve to same version
        let (ws2, ver2) = resolve_workspace("serde@1", None, false).unwrap();
        let ver2 = ver2.unwrap();

        assert_eq!(ver1, ver2, "bare and @1 should resolve to same latest 1.x");
        assert_eq!(
            ws1.path(),
            ws2.path(),
            "same version should reuse same cache directory"
        );
    });
}

#[test]
#[ignore = "network: requires crates.io API access"]
fn resolve_workspace_features_in_dir_name() {
    with_temp_cache(|_cache_path| {
        let (ws, _) = resolve_workspace("serde@1.0.200", Some("derive,alloc"), false).unwrap();
        let dir_name = ws.path().file_name().unwrap().to_string_lossy().to_string();
        // Features should be alpha-sorted
        assert_eq!(dir_name, "serde[1.0.200]+alloc+derive");
    });
}

#[test]
#[ignore = "network: requires crates.io API access"]
fn version_cache_file_created() {
    with_temp_cache(|cache_path| {
        let (_ws, _) = resolve_workspace("serde", None, false).unwrap();
        let version_cache = cache_path.join("versions").join("serde.json");
        assert!(
            version_cache.exists(),
            "versions/serde.json should be created after resolve"
        );

        // Should contain valid JSON with a "versions" array
        let content = std::fs::read_to_string(&version_cache).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(
            parsed.get("versions").and_then(|v| v.as_array()).is_some(),
            "version cache should contain a 'versions' array"
        );
    });
}

#[test]
#[ignore = "network: requires crates.io API access"]
fn clean_cache_removes_matching_dirs() {
    with_temp_cache(|cache_path| {
        // Create two serde workspaces with different versions
        let (ws1, _) = resolve_workspace("serde@1.0.200", None, false).unwrap();
        let (ws2, _) = resolve_workspace("serde@1.0.210", None, false).unwrap();
        let path1 = ws1.path().to_path_buf();
        let path2 = ws2.path().to_path_buf();

        assert!(path1.exists());
        assert!(path2.exists());
        assert!(cache_path.join("versions/serde.json").exists());

        clean_cache("serde").unwrap();

        assert!(!path1.exists(), "serde[1.0.200] should be removed");
        assert!(!path2.exists(), "serde[1.0.210] should be removed");
        assert!(
            !cache_path.join("versions/serde.json").exists(),
            "version cache should be removed"
        );
    });
}

#[test]
#[ignore = "network: requires crates.io API access + nightly toolchain"]
fn full_pipeline_header_shows_version() {
    with_temp_cache(|_cache_path| {
        let mut args = remote_args("serde");
        args.filter.compact = true;
        let output = run_api_pipeline(&args).expect("serde pipeline should succeed");
        // Header should contain version: "// crate serde[1.0.xxx]"
        assert!(
            output.starts_with("// crate serde[1.0."),
            "output should start with '// crate serde[1.0.', got:\n{}",
            output.lines().next().unwrap_or("")
        );
    });
}
