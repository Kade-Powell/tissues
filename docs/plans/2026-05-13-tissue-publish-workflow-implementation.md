# tissue crates.io Publish Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add full GitHub Actions CD publishing for `tissue-cli` to public crates.io.

**Architecture:** Keep the publish logic in a reusable `publish-cli-crates-io.yaml` workflow, then call it from release automation workflows. Use the default crates.io registry and document the install path in `README.md`.

**Tech Stack:** GitHub Actions, Cargo, Rust package metadata, crates.io.

---

### Task 1: crates.io Package Metadata

**Files:**
- Delete: `.cargo/config.toml`
- Modify: `Cargo.toml`
- Create: `LICENSE`

- [x] **Step 1: Remove private registry configuration**

Remove `.cargo/config.toml` so the package does not ship an internal registry URL.

- [x] **Step 2: Configure public package metadata**

Remove the private-registry `publish` restriction and add crates.io metadata such as license, keywords, and categories.

### Task 2: GitHub Actions Publish Workflows

**Files:**
- Create: `.github/workflows/publish-cli-crates-io.yaml`
- Modify: `.github/workflows/cd.yaml`
- Modify: `.github/workflows/cd-stable.yaml`

- [x] **Step 1: Add reusable/manual publish workflow**

Create a workflow that reads `CRATES_IO_TOKEN`, syncs the package version from the release tag, runs `cargo publish --allow-dirty`, and prints the install command.

- [x] **Step 2: Add main-branch CD workflow**

Create a `main` push workflow that calls Comcast's reusable tag-and-release workflow, then calls the Tissue crates.io publish workflow with the generated tag.

- [x] **Step 3: Add stable release workflow**

Create a stable release workflow that creates a stable tag from an `-rc` release and republishes the crates.io package with `allow_existing: true`.

### Task 3: Install Documentation And Verification

**Files:**
- Modify: `README.md`
- Modify: `tests/packaging.rs`

- [x] **Step 1: Document crates.io install setup**

Add `cargo install tissue-cli --locked`.

- [x] **Step 2: Verify manifests and package metadata**

Run `cargo metadata --no-deps --format-version 1`, `cargo package --allow-dirty --list`, `cargo fmt --check`, and `cargo test`.
