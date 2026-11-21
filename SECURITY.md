# Security Policy

## Reporting a Vulnerability

Do not disclose suspected vulnerabilities in public issues or discussions.

Report privately through GitHub's private vulnerability reporting feature:

<https://github.com/ko-k1/zsuggestion/security/advisories/new>

If private reporting is unavailable, contact the maintainer,
[@ko-k1](https://github.com/ko-k1), through GitHub without including exploit
details in a public channel.

## Scope and Response

- Affected versions: the current `main` branch and any tagged releases. The
  crate is not yet published on crates.io.
- Expected acknowledgement: within 7 days.
- Disclosure process: coordinated disclosure after a fix is available. Reporters
  are credited unless they prefer otherwise.

This file describes a reporting process; it does not replace the project's
technical security model. The daemon socket and state directories are
owner-private, and command output and environment values are not collected.
