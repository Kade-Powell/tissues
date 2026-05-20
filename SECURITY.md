# Security Policy

## Supported versions

Security fixes target the latest released version of `tissues` on crates.io and
the `main` branch.

## Reporting a vulnerability

Please do not open a public GitHub issue for a vulnerability, credential leak,
or token-handling bug.

Report security issues privately through GitHub Security Advisories for this
repository. Include:

- the affected version or commit,
- the operating system and terminal environment,
- reproduction steps,
- the expected impact,
- any relevant logs with tokens and secrets redacted.

## Credential handling

`tissues` uses the GitHub CLI for authentication and does not persist GitHub
tokens in its own configuration. Repo-local `.tissues/config.json` may name the
GitHub CLI account to use, but it must not contain tokens.
