//! `harness update` — print upgrade instructions for the current platform.

use anyhow::Result;
use std::cmp::Ordering;

pub fn run_update() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    if let Some(latest) = fetch_latest_release_tag() {
        let latest_ver = latest.strip_prefix('v').unwrap_or(&latest);
        match semver_compare(latest_ver, current) {
            Ordering::Greater => {
                println!("Update available: {latest} (installed: {current})");
                println!();
            }
            Ordering::Equal => {
                println!("You are on the latest release ({current}).");
                println!();
            }
            Ordering::Less => {
                println!("Installed {current} is newer than the latest GitHub release ({latest}).");
                println!();
            }
        }
    }

    println!("To update harness:");
    println!();
    #[cfg(target_os = "macos")]
    {
        println!("  curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash");
        println!();
        println!("Or from a clone:");
        println!("  git pull && cargo build --profile release-lto && install -m 755 target/release-lto/harness ~/.local/bin/harness");
    }
    #[cfg(target_os = "linux")]
    {
        println!("  curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash");
        println!();
        println!("Or from a clone:");
        println!("  git pull && cargo build --profile release-lto && install -m 755 target/release-lto/harness ~/.local/bin/harness");
    }
    #[cfg(windows)]
    {
        println!("  pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/install.ps1");
        println!();
        println!("Or from a clone:");
        println!("  git pull; cargo build --profile release-lto; Copy-Item target\\release-lto\\harness.exe $env:USERPROFILE\\.local\\bin\\");
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        println!("  git pull && cargo build --profile release-lto");
    }
    println!();
    println!("Ensure ~/.local/bin (or %USERPROFILE%\\.local\\bin) is on your PATH.");
    Ok(())
}

fn fetch_latest_release_tag() -> Option<String> {
    let out = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "https://api.github.com/repos/seanebones-lang/harness/releases/latest",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&out.stdout);
    body.lines()
        .find(|l| l.contains("\"tag_name\""))
        .and_then(|l| l.split('"').nth(3))
        .map(|s| s.to_string())
}

/// Compare semver-like strings including `-beta` / `-alpha` prereleases.
fn semver_compare(a: &str, b: &str) -> Ordering {
    let (a_nums, a_pre) = split_version(a);
    let (b_nums, b_pre) = split_version(b);
    let num_cmp = compare_numeric_parts(&a_nums, &b_nums);
    if num_cmp != Ordering::Equal {
        return num_cmp;
    }
    match (a_pre.as_deref(), b_pre.as_deref()) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(ap), Some(bp)) => ap.cmp(bp),
    }
}

fn split_version(v: &str) -> (Vec<u64>, Option<String>) {
    let (main, pre) = v.split_once('-').map_or((v, None), |(m, p)| (m, Some(p)));
    let nums = main.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    (nums, pre.map(str::to_string))
}

fn compare_numeric_parts(a: &[u64], b: &[u64]) -> Ordering {
    let len = a.len().max(b.len());
    for i in 0..len {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        match av.cmp(&bv) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_same_series_not_newer_than_release() {
        assert_eq!(
            semver_compare("0.1.2", "0.1.2-beta"),
            Ordering::Greater,
            "release should beat prerelease at same numeric version"
        );
    }

    #[test]
    fn newer_patch_detected() {
        assert_eq!(semver_compare("0.1.3", "0.1.2-beta"), Ordering::Greater);
    }

    #[test]
    fn equal_versions() {
        assert_eq!(semver_compare("0.1.2-beta", "0.1.2-beta"), Ordering::Equal);
    }
}
