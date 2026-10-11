> Historical unpublished candidate. The `v1.4.0-beta.1` tag is retained; its Windows release smoke failed on fixture encoding, and no complete release was published. Use [the corrected beta.2 candidate notes](RELEASE_NOTES_v1.4.0-beta.2.md).

# Harness 1.4.0-beta.1

This developer prerelease brings the current personal-work workflows into
version-pinned CLI downloads. Harness is free MIT source. The default binary
includes the pinned Apache-2.0 DeadBolt dependency; other dependencies retain
their own licenses. Keep the accompanying license files when redistributing.

Start with [the quickstart](QUICKSTART.md). Download the binary for your machine,
verify it against `checksums.txt`, and prepare an outcome with
`harness -C /path/to/work work --kind software --prepare-only`. That first step
needs no account or API key. Choose your exact provider/model in `setup` for
actual model work; a local backend or your provider account is required.

## Included

- Persistent work briefs for software, research, documents, websites, automation
  and Apple projects; explicit workspace selection; saved acceptance checks and
  observed progress; resumable sessions.
- Workspace skill discovery and bundled workflow installation that preserves
  user edits; bounded source-labeled document extraction with coverage limits.
- Provider-neutral routing, explicit ordered fallbacks and credential-variable
  references. No credential-based provider preference or implicit fallback.
- Real image bytes in the OpenAI-compatible transport and saved sessions.
  Native adapters without multipart support reject attachments explicitly.
- DeadBolt admission after review and immediately before registered tool effects.
  Policy or lease changes during review stop the action; declining review
  preserves a one-shot. Legacy dispatch refuses tools marked exact-only.
- A release matrix that tests the actual optimized binary on five native targets,
  including isolated CLI/HTTP/build/work and synthetic image/session checks.
- Source archive, installation scripts, license information and a manifest
  recording the exact commit, targets, asset sizes and SHA-256 hashes.

## Evidence and limits

Exact-source CI and Coverage, all five release artifact jobs and package hashes
must pass before publication. Available-Mac checks cover installation, terminal,
browser and actual tools; release notes and the acceptance receipt record what
was performed. Synthetic providers validate runtime mechanics; they do not
benchmark model quality or autonomous project completion.

This is a **prerelease**, not stable cross-platform interactive acceptance.
Manual/fresh-machine tests on every advertised OS, signed/notarized desktop
packages and every optional integration remain separate gates. CLI downloads do
not include the optional Tauri or VS Code packages. The VS Code source metadata
uses base 1.4.0; marketplace prerelease publication is separate.

Tools run with your account's authority. Plan review and DeadBolt admission do
not establish arbitrary-code isolation; autoformat/test hooks and code outside
the dispatcher retain their documented boundary. Harness does not issue
payload-bound DeadBolt grants automatically. See [DEADBOLT.md](DEADBOLT.md).

The historical v1.3.0 release and tag are retained. GitHub's unqualified
latest-release link may still select that release. Use the explicit
[v1.4.0-beta.1 download](https://github.com/seanebones-lang/harness/releases/tag/v1.4.0-beta.1)
and a pinned installer argument for this candidate.
