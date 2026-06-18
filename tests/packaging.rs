use std::fs;

#[test]
fn package_and_binary_use_public_repo_name() {
    let manifest = fs::read_to_string("Cargo.toml").expect("read Cargo.toml");

    assert!(manifest.contains("name = \"tissues\""));
    assert!(manifest.contains("[lib]\nname = \"tissues\""));
    assert!(manifest.contains("[[bin]]\nname = \"tissues\""));
}

#[test]
fn package_is_configured_for_public_crates_io() {
    let manifest = fs::read_to_string("Cargo.toml").expect("read Cargo.toml");

    assert!(!manifest.contains("publish = ["));
    assert!(manifest.contains("license = "));
    assert!(
        !fs::exists(".cargo/config.toml").expect("check .cargo/config.toml"),
        "public crates.io packages must not ship a private registry config"
    );
}

#[test]
fn crates_io_workflow_is_reusable_publish_step() {
    let workflow = fs::read_to_string(".github/workflows/publish-cli-crates-io.yaml")
        .expect("read crates.io publish workflow");

    assert!(workflow.contains("workflow_call:"));
    assert!(!workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("checkout_ref:"));
    assert!(workflow.contains("dry_run:"));
    assert!(workflow.contains("cargo publish --dry-run --allow-dirty"));
}

#[test]
fn ci_workflow_checks_format_tests_package_and_actions() {
    let workflow = fs::read_to_string(".github/workflows/ci.yaml").expect("read CI workflow");

    assert!(workflow.contains("pull_request:"));
    assert!(workflow.contains("Commit message checks"));
    assert!(workflow.contains("check-conventional-commit.py"));
    assert!(workflow.contains("runs-on: ubuntu-latest"));
    assert!(workflow.contains("rustup component add rustfmt"));
    assert!(workflow.contains("cargo fmt --check"));
    assert!(workflow.contains("cargo test"));
    assert!(workflow.contains("prek run --all-files"));
    assert!(workflow.contains("prek run --all-files --hook-stage pre-push"));
    assert!(workflow.contains("cargo publish --dry-run --allow-dirty"));
    assert!(workflow.contains("raven-actions/actionlint@v2"));
}

#[test]
fn prek_enforces_conventional_commit_messages() {
    let config =
        fs::read_to_string(".pre-commit-config.yaml").expect("read prek/pre-commit config");
    let checker = fs::read_to_string(".github/scripts/check-conventional-commit.py")
        .expect("read conventional commit checker");

    assert!(config.contains("commit-msg"));
    assert!(config.contains("pre-push"));
    assert!(config.contains("check-conventional-commit.py"));
    assert!(config.contains("cargo fmt --check"));
    assert!(config.contains("cargo test --test packaging"));
    assert!(checker.contains("ALLOWED_TYPES"));
    assert!(checker.contains("HEADER_RE"));
}

#[test]
fn release_workflow_publishes_from_public_github_actions() {
    let workflow = fs::read_to_string(".github/workflows/cd.yaml").expect("read release workflow");

    assert!(workflow.contains("push:"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("version_tag:"));
    assert!(workflow.contains("VERSION_TAG_OVERRIDE:"));
    assert!(workflow.contains("python3 .github/scripts/plan-release.py"));
    assert!(workflow.contains("python3 .github/scripts/set-cargo-version.py"));
    assert!(workflow.contains("cargo test"));
    assert!(workflow.contains("chore(release): ${{ steps.plan.outputs.tag }} [skip ci]"));
    assert!(workflow.contains("git tag -a \"${{ steps.plan.outputs.tag }}\""));
    assert!(workflow.contains("git push --follow-tags origin HEAD:main"));
    assert!(workflow.contains("gh release create \"${{ steps.plan.outputs.tag }}\""));
    assert!(!workflow.contains("--target \"${{ steps.plan.outputs.tag }}\""));
    assert!(workflow.contains("./.github/workflows/publish-cli-crates-io.yaml"));
    assert!(workflow.contains("version_tag: ${{ needs.create-release.outputs.version_tag }}"));
    assert!(workflow.contains("runner_label: ubuntu-latest"));
    assert!(workflow.contains("CRATES_IO_TOKEN: ${{ secrets.CRATES_IO_TOKEN }}"));
    assert!(!fs::exists(".github/workflows/cd-stable.yaml").expect("check cd-stable workflow"));
}

#[test]
fn release_planner_uses_conventional_commits() {
    let planner =
        fs::read_to_string(".github/scripts/plan-release.py").expect("read release planner");

    assert!(planner.contains("BREAKING[- ]CHANGE"));
    assert!(planner.contains("VERSION_TAG_OVERRIDE"));
    assert!(planner.contains("tag_exists"));
    assert!(planner.contains("rstrip(\"\\n\")"));
    assert!(planner.contains("match.group(\"type\") == \"feat\""));
    assert!(planner.contains("return \"patch\""));
    assert!(planner.contains("release-notes.md"));
}

#[test]
fn package_metadata_points_at_public_repository() {
    let manifest = fs::read_to_string("Cargo.toml").expect("read Cargo.toml");

    assert!(manifest.contains("repository = \"https://github.com/Kade-Powell/tissues\""));
}

#[test]
fn repository_is_set_up_for_public_contributions() {
    for path in [
        "CONTRIBUTING.md",
        "CODE_OF_CONDUCT.md",
        "SECURITY.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
        ".github/ISSUE_TEMPLATE/bug_report.yml",
        ".github/ISSUE_TEMPLATE/feature_request.yml",
        ".github/ISSUE_TEMPLATE/docs.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/dependabot.yml",
    ] {
        assert!(fs::exists(path).expect("check contribution file"), "{path}");
    }

    let contributing = fs::read_to_string("CONTRIBUTING.md").expect("read CONTRIBUTING.md");
    assert!(contributing.contains("Conventional Commits"));
    assert!(contributing.contains("cargo fmt --check"));
    assert!(contributing.contains("cargo test"));
    assert!(contributing.contains("tui-realm"));
}

#[test]
fn readme_install_instructions_do_not_pin_a_stale_version() {
    let readme = fs::read_to_string("README.md").expect("read README.md");

    assert!(readme.contains("cargo install tissues --locked"));
    assert!(!readme.contains("cargo install tissues --version \"0.2.0\""));
}

#[test]
fn workflows_do_not_reference_internal_infra() {
    for entry in fs::read_dir(".github/workflows").expect("read workflows directory") {
        let path = entry.expect("read workflow entry").path();
        let workflow = fs::read_to_string(&path).expect("read workflow");

        assert!(!workflow.contains("comcast-ubuntu-latest"), "{path:?}");
        assert!(!workflow.contains("comcast-zorrillo"), "{path:?}");
        assert!(!workflow.contains("gha-reusable-workflows"), "{path:?}");
    }
}
