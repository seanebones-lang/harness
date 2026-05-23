# Security Policy

## Supported Versions

We currently support the latest version on the `main` branch.

| Version | Supported          |
| ------- | ------------------ |
| latest  | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly:

1. **Do not** open a public GitHub issue.
2. Email the maintainer at a private channel or use GitHub's private vulnerability reporting feature.
3. Provide as much detail as possible (steps to reproduce, affected versions, potential impact).

We will respond within 48 hours and work with you on a fix.

## Threat Model

Harness is a local-first tool. The main security considerations are:

- Shell command execution (`shell` tool)
- File system access
- Network requests to LLM providers
- MCP server connections

See `docs/threat-model.md` (if present) for more details.