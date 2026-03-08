use std::process::Command;

#[test]
fn cli_smoke_test() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--",
            "test-fixture",
            "--manifest-path",
            "test_fixture/Cargo.toml",
            "--recursive",
        ])
        .output()
        .expect("Failed to run cargo-brief");

    assert!(output.status.success(), "cargo-brief exited with error");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("// crate test_fixture"), "has crate header");
    assert!(stdout.contains("pub struct PubStruct"), "has PubStruct");
    assert!(stdout.contains("pub enum PlainEnum"), "has PlainEnum");
    assert!(stdout.contains("pub union MyUnion"), "has MyUnion");
    assert!(stdout.contains("pub static GLOBAL_COUNT"), "has static");
}

#[test]
fn cli_self_keyword_test() {
    // Run from the project root — "self" should resolve to "cargo-brief"
    let output = Command::new("cargo")
        .args(["run", "--", "self", "--depth", "0"])
        .output()
        .expect("Failed to run cargo-brief with self");

    assert!(
        output.status.success(),
        "cargo-brief self exited with error: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("// crate cargo_brief"),
        "self should resolve to cargo-brief crate, got:\n{stdout}"
    );
}

#[test]
fn cli_self_module_syntax_test() {
    // "self::cli" should resolve to cargo-brief's cli module
    let output = Command::new("cargo")
        .args(["run", "--", "self::cli"])
        .output()
        .expect("Failed to run cargo-brief with self::cli");

    assert!(
        output.status.success(),
        "cargo-brief self::cli exited with error: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("BriefArgs"),
        "self::cli should show BriefArgs, got:\n{stdout}"
    );
}
