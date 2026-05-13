# skunkwork Publish Workflow Design

Date: 2026-05-13

## Goal

Publish the `skunkwork` Rust binary crate to the same internal Artifactory Cargo registry used by Cola so users can install it with `cargo install`.

## Registry

Skunkwork uses the internal `artifactory` Cargo registry:

```toml
[registries.artifactory]
index = "sparse+https://artifactory.comcast.com/artifactory/api/cargo/titan-cargo/index/"
```

Credentials are supplied by Cargo's token credential provider. CI reads the token from `ARTIFACTORY_TITAN_CARGO_PASSWORD`.

## Release Flow

Pushing code changes to `main` runs a CD workflow that calls Comcast's reusable tag-and-release workflow, then publishes the generated release tag to Artifactory.

The publish job derives the Cargo package version from the release tag by stripping any leading non-numeric prefix. For example, `v0.2.0-rc.1` publishes package version `0.2.0-rc.1`.

## Stable Flow

Publishing a stable GitHub release from an `-rc` tag creates the stable tag through the same reusable workflow pattern used by Cola, then republishes `skunkwork` with the stable version. Already-published versions are treated as success for this stable path.

## Manual Flow

The publish workflow also supports `workflow_dispatch` for manual publishing. The operator can provide a version tag, runner label, timeout, and whether an existing package version should be accepted.

## Install Experience

Users configure the Artifactory Cargo registry token locally, then install with:

```bash
cargo install --registry artifactory skunkwork --locked
```

Specific versions are installed with:

```bash
cargo install --registry artifactory skunkwork --version "0.1.0" --locked
```

## Out Of Scope

- Publishing to crates.io.
- Binary archive uploads outside Cargo.
- Homebrew, npm, or platform package manager distribution.
