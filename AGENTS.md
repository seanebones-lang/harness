# Working on Harness

## Orientation

Read [README.md](README.md), [SECURITY.md](SECURITY.md) and the relevant workspace
manifest before editing. This is an MIT personal-work assistant; the current
1.4.0-beta.2 source is a prerelease for developer evaluation. Historical launch
and commercial documents do not override that status. Preserve unrelated edits.
Instructions under `docs/AGENTS.md` apply when working within that directory.

## Build and verify

The repository pins Rust 1.95.0. Python 3 is needed for acceptance helpers and
Node/npm for the local web interface tests.

```sh
cargo build --locked
./target/debug/harness --version
./target/debug/harness --help
make verify
make web-test
```

`make verify` runs formatting, strict all-feature workspace lint/tests, synthetic
acceptance checks and a no-default-features lint pass. These checks need no model
API key. Dependencies can require network access. `make supply-chain` runs
`cargo deny check` when cargo-deny is installed. Use the checked-out binary,
rather than an unrelated PATH installation, when checking commands or flags.
[The Makefile](Makefile) and [CI](.github/workflows/ci.yml) define the full gates.

## Tool and provider contracts

- The user selects the exact `provider:model` route and ordered fallbacks.
  Credentials must not rank providers or insert a route.
- Use mocked providers for automated tests. Live model calls, uploads, external
  messages and paid APIs require task authorization; local tests do not imply it.
- `harness build ... --prepare-only` and `harness work ... --prepare-only` prepare
  a local brief without model calls. A brief or progress note is not verification
  that its acceptance criteria passed.
- Preserve file/shell approval, workspace and cancellation boundaries. Tool
  permission controls are not an arbitrary-code OS sandbox.
- Carry actual image bytes through supported transports or reject explicitly;
  do not replace an attachment with its filename and claim visual support.
- Preserve full tool receipts and resumable sessions on incomplete or failed runs.
  Keep credentials out of output, fixtures and Git.

## Release and evidence

Use explicitly versioned prerelease downloads for this source; the unqualified
latest-release installer may select historical v1.3.0. A source merge is separate
from package publication. Keep synthetic checks, installed behavior, live-provider
quality and optional platform coverage separate in the handoff. Record commands,
results and untested gates without upgrading a beta to a production claim.
