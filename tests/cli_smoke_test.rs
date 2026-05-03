//! CLI subprocess smoke tests (no API keys).

use std::process::Command;

fn harness_bin() -> String {
    env!("CARGO_BIN_EXE_harness").to_string()
}

#[test]
fn harness_version_exits_zero() {
    let out = Command::new(harness_bin())
        .args(["--version"])
        .output()
        .expect("spawn harness --version");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("harness"),
        "expected harness in version output: {stdout}"
    );
}

#[test]
fn harness_help_exits_zero() {
    let out = Command::new(harness_bin())
        .args(["--help"])
        .output()
        .expect("spawn harness --help");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Usage") || stdout.contains("harness"),
        "unexpected help: {stdout}"
    );
}
