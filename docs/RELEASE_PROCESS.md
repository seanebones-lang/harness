# Release Process

This document describes how to cut a new release of Harness.

## Prerequisites

- All CI checks on `main` must be green
- `docs/PUBLIC_RELEASE.md` checks must pass
- You have write access to the repository

## Steps

1. **Ensure `main` is ready**

   ```bash
   git checkout main
   git pull origin main
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all
   ```

2. **Create and push a version tag**

   ```bash
   git tag -a v0.1.2-beta -m "v0.1.2-beta - Public beta promotion polish"
   git push origin v0.1.2-beta
   ```

3. **GitHub Actions will automatically**:
   - Build binaries for macOS (arm64 + x86_64), Linux (x86_64 + aarch64), and Windows
   - Create a GitHub Release with all binaries attached
   - Generate release notes from commits

4. **After the release is published**:
   - Verify the binaries are downloadable
   - Test the install script:
     ```bash
     curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
     ```
   - Announce the release

## Versioning

We follow semantic versioning:
- `v0.x.y` — Initial development / breaking changes allowed
- `v1.0.0` — First stable release

## Current release (v0.1.2-beta)

Workspace version is **`0.1.2-beta`**. After tagging, the install scripts serve prebuilt binaries from GitHub Releases. Run `scripts/update-homebrew-sha.sh v0.1.2-beta` after publishing to refresh the Homebrew tap.

## Rollback

If a release has issues, create a new patch release (`v0.1.1`) rather than deleting the tag.
## Future: cargo-dist

We are considering migrating to [cargo-dist](https://github.com/axodotdev/cargo-dist) for even better release automation, Homebrew taps, and installer generation. The `dist-workspace.toml` file is already present as a starting point.
