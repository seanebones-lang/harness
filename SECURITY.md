# Security Policy — NextEleven Harness

## Supported Versions

| Version     | Supported |
| ----------- | --------- |
| 0.1.2-beta  | Yes       |
| main branch | Yes       |

## Reporting a Vulnerability

1. **Do not** open a public GitHub issue for security-sensitive reports.
2. Use [GitHub private vulnerability reporting](https://github.com/seanebones-lang/harness/security/advisories/new) or contact maintainers via the GitHub organization profile.
3. Include steps to reproduce, affected versions, and potential impact.

We acknowledge within **24 hours** and aim for a full assessment within **72 hours**, with coordinated disclosure before public write-ups. Target fix window: **14 days** after confirmation.

## Known Advisories

None at this time. P0 findings from the May 2026 peer review were remediated — see [`docs/PEER_REVIEW_AUDIT.md`](docs/PEER_REVIEW_AUDIT.md).

## Security Design Highlights

- **Workspace sandbox**: file, git, notebook, and database paths jailed under the workspace root (strict by default)
- **Confirm gate / plan mode**: destructive tools pause for approval (`--plan` / `[approval]`)
- **HTTP bearer auth**: `harness serve` `/api/*` requires token; bind loopback by default
- **Daemon IPC**: Unix socket (macOS/Linux) or loopback TCP (Windows) + token file
- **Optional tools default off**: `database`, `notebook`, `docker`, `computer_use`
- **MCP**: command allowlist; sampling approval (TUI or auto)
- **Swarm workers**: optional tool allowlist + wall-clock timeout
- **Supply-chain**: `cargo deny` / advisories when CI is configured

## Threat Model

Full detail: [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md) (v2, 2026-08-03).

## License

**Proprietary — NextEleven LLC.** See [`LICENSE`](LICENSE). Unauthorized use or redistribution is prohibited.

## Public repository hygiene

- Never commit API keys, `.env` / `.envrc`, tokens, or private keys
- Report secrets found in git history privately (rotate the key first)
- Redact keys from bug reports and screenshots
- Local state lives under `~/.harness/` (not the git tree)
