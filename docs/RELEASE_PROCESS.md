# Release Process

This document describes how to cut a new release of NextEleven Harness.

**License:** proprietary NextEleven LLC — public GitHub = POC visibility only. **Not MIT.**

## Prerequisites

- All CI checks on `main` must be green
- `docs/PUBLIC_RELEASE.md` checks must pass
- Workspace `version` in root `Cargo.toml` matches the tag (no `v` prefix in Cargo; tag is `vX.Y.Z`)
- You have write access to the repository

## Steps

1. **Ensure `main` is ready**

   ```bash
   git checkout main
   git pull origin main
   cargo fmt --all -- --check
   cargo clippy -p harness --bin harness -- -D warnings
   cargo test --bin harness
   ```

2. **Create and push a version tag**

   ```bash
   git tag -a v1.3.0 -m "v1.3.0 - Public POC proprietary cut"
   git push origin v1.3.0
   ```

3. **GitHub Actions will automatically** (when billing/workflows enabled):
   - Build binaries for macOS (arm64 + x86_64), Linux (x86_64 + aarch64), and Windows
   - Create a GitHub Release with all binaries attached
   - Generate release notes from commits

4. **After the release is published**:
   - Verify the binaries are downloadable
   - Attach / link [`docs/RELEASE_NOTES_v1.3.0.md`](RELEASE_NOTES_v1.3.0.md)
   - Test the install script when prebuilts exist:
     ```bash
     curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
     ```
   - Run `bash scripts/update-homebrew-sha.sh v1.3.0` after multi-arch artifacts exist
   - Announce the release (restate proprietary / POC terms)

## Versioning

We follow semantic versioning:
- `v0.x.y` / early `v1.x` POC — breaking changes allowed under proprietary license
- First **stable** product cut tracked separately (REL-01 + prebuilts); not the same as “open source 1.0”

## Current release (v1.3.0)

Workspace version is **`1.3.0`**. Notes: [`RELEASE_NOTES_v1.3.0.md`](RELEASE_NOTES_v1.3.0.md).  
Prior beta tag history includes `v0.1.2-beta` (macOS arm64 partial artifacts only).

## Rollback

If a release has issues, create a new patch release (`v1.3.1`) rather than deleting the tag.

## Future: cargo-dist

We are considering migrating to [cargo-dist](https://github.com/axodotdev/cargo-dist) for release automation, Homebrew taps, and installer generation. The `dist-workspace.toml` file is already present as a starting point.
