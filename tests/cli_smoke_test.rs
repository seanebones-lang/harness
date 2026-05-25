//! CLI subprocess smoke tests (no API keys).

use std::process::Command;
use std::time::{Duration, Instant};

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

#[test]
fn lightweight_commands_exit_quickly_without_stdin() {
    let bin = harness_bin();
    for args in [
        &["sessions"][..],
        &["doctor"][..],
        &["status"][..],
        &["--help"][..],
    ] {
        let start = Instant::now();
        let out = Command::new(&bin)
            .env("HARNESS_SKIP_LSP", "1")
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("spawn harness {args:?}: {e}"));
        assert!(
            out.status.success(),
            "harness {:?} failed: stderr={}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "harness {:?} took {:?} — expected lightweight startup",
            args,
            start.elapsed()
        );
    }
}
