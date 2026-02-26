use std::process::Command;

#[test]
fn binary_help_exits_successfully() {
    let bin = assert_cmd::cargo::cargo_bin!("signal-desktop-only");
    let output = Command::new(bin)
        .arg("--help")
        .output()
        .expect("failed to run binary --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Interactive Signal Docker"));
}
