use std::fs;

#[test]
fn package_name_is_registry_safe_while_binary_stays_tissue() {
    let manifest = fs::read_to_string("Cargo.toml").expect("read Cargo.toml");

    assert!(manifest.contains("name = \"tissue-cli\""));
    assert!(manifest.contains("[lib]\nname = \"tissue\""));
    assert!(manifest.contains("[[bin]]\nname = \"tissue\""));
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
fn crates_io_workflow_supports_manual_dry_run() {
    let workflow = fs::read_to_string(".github/workflows/publish-cli-crates-io.yaml")
        .expect("read crates.io publish workflow");

    assert!(workflow.contains("dry_run:"));
    assert!(workflow.contains("cargo publish --dry-run --allow-dirty"));
    assert!(workflow.contains("Manual workflow_dispatch publishes must use dry_run=true."));
}

#[test]
fn ci_workflow_checks_format_tests_package_and_actions() {
    let workflow = fs::read_to_string(".github/workflows/ci.yaml").expect("read CI workflow");

    assert!(workflow.contains("pull_request:"));
    assert!(workflow.contains("runs-on: ubuntu-latest"));
    assert!(workflow.contains("rustup component add rustfmt"));
    assert!(workflow.contains("cargo fmt --check"));
    assert!(workflow.contains("cargo test"));
    assert!(workflow.contains("cargo publish --dry-run --allow-dirty"));
    assert!(workflow.contains("raven-actions/actionlint@v2"));
}

#[test]
fn release_workflow_publishes_from_public_github_actions() {
    let workflow = fs::read_to_string(".github/workflows/cd.yaml").expect("read release workflow");

    assert!(workflow.contains("release:"));
    assert!(workflow.contains("types: [published]"));
    assert!(workflow.contains("./.github/workflows/publish-cli-crates-io.yaml"));
    assert!(workflow.contains("version_tag: ${{ github.event.release.tag_name }}"));
    assert!(workflow.contains("runner_label: ubuntu-latest"));
    assert!(workflow.contains("CRATES_IO_TOKEN: ${{ secrets.CRATES_IO_TOKEN }}"));
    assert!(!fs::exists(".github/workflows/cd-stable.yaml").expect("check cd-stable workflow"));
}

#[test]
fn package_metadata_points_at_public_repository() {
    let manifest = fs::read_to_string("Cargo.toml").expect("read Cargo.toml");

    assert!(manifest.contains("repository = \"https://github.com/Kade-Powell/tissues\""));
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
