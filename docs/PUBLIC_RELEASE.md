# Optional public distribution

Harness is MIT licensed. Source is public; the owner's acceptance target is personal use, documented in [PERSONAL_USE.md](PERSONAL_USE.md).

Before publishing a new versioned binary, run locked workspace checks and the installation/runtime acceptance scripts. Verify each advertised platform and architecture, checksums, package license contents, and clean-machine installation. Do not reuse a version tag to replace an old artifact. Signed desktop distribution and marketplace publication require their own verification.

These packaging checks do not prevent personal use of a locally built, verified executable. Historical gate results remain in [RELEASE_STATUS.md](RELEASE_STATUS.md).
