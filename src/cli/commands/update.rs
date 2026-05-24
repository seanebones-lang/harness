//! `harness update` — print upgrade instructions for the current platform.

use anyhow::Result;

pub fn run_update() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    if let Some(latest) = fetch_latest_release_tag() {
        let latest_ver = latest.strip_prefix('v').unwrap_or(&latest);
        if latest_ver != current {
            println!("Update available: {latest} (installed: {current})");
            println!();
        } else {
            println!("You are on the latest release ({current}).");
            println!();
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
