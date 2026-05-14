use std::process::Command;

#[test]
fn help_uses_tissues_branding() {
    let output = Command::new(env!("CARGO_BIN_EXE_tissues"))
        .arg("--help")
        .output()
        .expect("run tissues --help");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("help output is utf-8");
    assert!(stdout.contains("Usage: tissues"));
    let old_name = ["skunk", "work"].concat();
    assert!(!stdout.to_lowercase().contains(&old_name));
}
