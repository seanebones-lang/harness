# Release Policy

This document defines how Harness releases are managed.

## Versioning

Harness follows [Semantic Versioning 2.0.0](https://semver.org/):

- **Major** (`1.0.0`): Breaking changes
- **Minor** (`0.2.0`): New features, non-breaking
- **Patch** (`0.1.1`): Bug fixes

During the `0.x` phase, breaking changes may occur in minor releases.

## Release Cadence

- Releases are cut when there is meaningful value to users (new features, important fixes, or stability improvements).
- There is no fixed schedule.

## Release Requirements

Before tagging a release, the following must be true:

1. `main` branch CI is green (`cargo fmt`, `clippy`, `test`, `build`)
2. `docs/PUBLIC_RELEASE.md` checklist has been reviewed
3. No known critical bugs in the release candidate
4. Changelog has been updated (`CHANGELOG.md`)

## Tagging Process

1. Ensure all release requirements are met
2. Create an annotated tag:
   ```bash
   git tag -a v0.2.0 -m "v0.2.0 — Description of changes"
   ```
3. Push the tag:
   ```bash
   git push origin v0.2.0
   ```

The Release workflow will build binaries for the five native targets and stage a **draft** GitHub Release. Publication is a separate verified step; a successful build never publishes a release by itself.

## Supported Platforms

- macOS (arm64 + x86_64)
- Linux (x86_64 + aarch64)
- Windows (x86_64)

## First Release

The initial public release was `v0.1.0` (May 2026).

## Rollback

If a release has serious issues, a new patch release will be issued. Tags are not deleted.
## Developer prereleases

A prerelease such as `1.4.0-beta.2` may be distributed for evaluation after exact-commit CI/Coverage, all five native artifact builds/tests/synthetic checks, license packaging, checksum verification, and the installer/first-use checks on the available Mac pass. Mark it as a GitHub prerelease, keep its explicit version in installation examples, and list unperformed interactive/fresh-machine/live-provider checks. Automated hosted runners provide platform evidence, not a claim of manual acceptance on every operating system. Stable promotion still requires the broader interactive and clean-machine acceptance described in RELEASE_PROCESS.md. Optional desktop/editor source metadata tracks the candidate; the VS Code package uses the base `1.4.0` because extension prerelease channels use marketplace metadata. Neither optional client is distributed by the CLI release workflow.
