# skunkwork Publish Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add full GitHub Actions CD publishing for `skunkwork` to the internal Artifactory Cargo registry.

**Architecture:** Keep the publish logic in a reusable `publish-cli-artifactory.yaml` workflow, then call it from release automation workflows. Store non-secret registry index configuration in `.cargo/config.toml`, and document the install path in `README.md`.

**Tech Stack:** GitHub Actions, Cargo, Rust package metadata, Comcast Artifactory Cargo registry.

---

### Task 1: Cargo Registry And Package Metadata

**Files:**
- Create: `.cargo/config.toml`
- Modify: `Cargo.toml`

- [x] **Step 1: Add the Artifactory Cargo registry index**

Create `.cargo/config.toml` with the `artifactory` sparse index and token credential provider.

- [x] **Step 2: Restrict package publishing to the Artifactory registry**

Set `publish = ["artifactory"]` in `Cargo.toml` and add package description/repository metadata.

### Task 2: GitHub Actions Publish Workflows

**Files:**
- Create: `.github/workflows/publish-cli-artifactory.yaml`
- Create: `.github/workflows/cd.yaml`
- Create: `.github/workflows/cd-stable.yaml`

- [x] **Step 1: Add reusable/manual publish workflow**

Create a workflow that configures Artifactory credentials, syncs the package version from the release tag, runs `cargo publish --registry artifactory --allow-dirty`, and prints the install command.

- [x] **Step 2: Add main-branch CD workflow**

Create a `main` push workflow that calls Comcast's reusable tag-and-release workflow, then calls the Skunkwork publish workflow with the generated tag.

- [x] **Step 3: Add stable release workflow**

Create a stable release workflow that creates a stable tag from an `-rc` release and republishes the Artifactory package with `allow_existing: true`.

### Task 3: Install Documentation And Verification

**Files:**
- Modify: `README.md`

- [x] **Step 1: Document Artifactory install setup**

Add the Artifactory registry environment variables and `cargo install --registry artifactory skunkwork --locked`.

- [x] **Step 2: Verify manifests and package metadata**

Run `cargo metadata --no-deps --format-version 1` and a YAML parser over the workflow files.
