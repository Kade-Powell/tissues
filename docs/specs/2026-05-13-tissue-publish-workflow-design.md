# tissue crates.io Publish Workflow Design

Date: 2026-05-13

## Goal

Publish the `tissue-cli` Rust package to public crates.io so users can install the `tissue` binary with `cargo install tissue-cli`.

## Registry

Tissue uses the default crates.io registry. CI reads the publish token from `CRATES_IO_TOKEN` and passes it to Cargo through `CARGO_REGISTRY_TOKEN`.

## Release Flow

Pushing code changes to `main` runs a CD workflow that calls Comcast's reusable tag-and-release workflow, then publishes the generated release tag to crates.io.

The publish job derives the Cargo package version from the release tag by stripping any leading non-numeric prefix. For example, `v0.2.0-rc.1` publishes package version `0.2.0-rc.1`.

## Stable Flow

Publishing a stable GitHub release from an `-rc` tag creates the stable tag through the same reusable workflow pattern used by Cola, then republishes `tissue-cli` with the stable version. Already-published versions are treated as success for this stable path.

## Manual Flow

The publish workflow also supports `workflow_dispatch` for manual publishing. The operator can provide a version tag, runner label, timeout, and whether an existing package version should be accepted.

## Install Experience

Users install the latest release with:

```bash
cargo install tissue-cli --locked
```

Specific versions are installed with:

```bash
cargo install tissue-cli --version "0.1.0" --locked
```

## Out Of Scope

- Binary archive uploads outside Cargo.
- Homebrew, npm, or platform package manager distribution.
