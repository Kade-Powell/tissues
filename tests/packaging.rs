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
    assert!(workflow.contains("runs-on: comcast-ubuntu-latest"));
    assert!(workflow.contains("cargo fmt --check"));
    assert!(workflow.contains("cargo test"));
    assert!(workflow.contains("cargo publish --dry-run --allow-dirty"));
    assert!(workflow.contains("raven-actions/actionlint@v2"));
}
