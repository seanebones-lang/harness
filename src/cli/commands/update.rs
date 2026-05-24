//! `harness update` — print upgrade instructions for the current platform.

use anyhow::Result;

pub fn run_update() -> Result<()> {
    println!("To update harness to the latest release:");
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
