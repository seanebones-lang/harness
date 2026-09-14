# Release process

Harness is distributed under the proprietary NextEleven LLC evaluation license. Stable release acceptance is recorded in [PRODUCTION_READINESS.md](PRODUCTION_READINESS.md) and [RELEASE_STATUS.md](RELEASE_STATUS.md).

## Prepare a candidate

1. Work on `main`, preserve unrelated work, and pass the local and remote gates in [PUBLIC_RELEASE.md](PUBLIC_RELEASE.md).
2. Select a new version at release time and align the workspace, desktop, extension, release notes, and image metadata. Never move an existing release tag to newer code.
3. Require green CI and Coverage for the exact candidate commit on `main`, including the installer contracts and synthetic agent/HTTP smoke. Synthetic provider tests do not replace real-provider and interactive acceptance.
4. Create an annotated `vX.Y.Z` tag matching the Cargo workspace version and push it only after the release checklist is satisfied.

## Build and stage artifacts

The Release workflow accepts an existing tag, checks that it belongs to `main`, verifies its Cargo version and successful latest CI and Coverage runs for that commit, and checks out its immutable commit for every build. A manual dispatch must supply that existing tag; it does not silently attach current `main` binaries to an older version.

Each of five native runners builds the pinned, locked optimized candidate and runs workspace tests plus binary, isolated CLI, and synthetic provider/HTTP checks. The runners cover macOS arm64/x86_64, Linux arm64/x86_64, and Windows x86_64. The matrix uses supported labels from [GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

Only a completely successful matrix stages a **draft** GitHub Release with all five artifacts and `checksums.txt`. Missing platform artifacts prevent staging. GitHub Actions billing must be enabled; a job that never starts supplies no validation evidence.

## Publish after acceptance

1. Verify all draft downloads and SHA-256 checksums, real provider round trips, interactive TUI/browser/editor behavior, and clean-machine install on the supported systems.
2. Include final release notes and explicit limitations; publish the draft when those gates pass.
3. Run `bash scripts/update-homebrew-sha.sh vX.Y.Z`. This verifies all four Unix downloads against the release manifest before updating every formula checksum and version. Review and commit the formula.
4. Rehearse the public installers against the published version. Both require valid checksums. A version-pinned source fallback clones that tag, never unrelated current-directory code.

For CI or a local current-checkout install, use `HARNESS_INSTALL_SOURCE=1` and a disposable `HARNESS_INSTALL_DIR`. Leave provider selection to `harness setup`; installers do not seed a preferred vendor or model.

## Rollback

Retain the preceding version and its checksums. If a release is faulty, stop promotion and ship a new patch version after validation. Never overwrite an existing version with a different binary or silently replace a tag.
