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
