use std::process::Command;

#[test]
fn command_failure_is_a_diagnostic_on_stderr() {
    let output = Command::new(env!("CARGO_BIN_EXE_kurumi-containerd"))
        .env("HOME", "/dev/null")
        .arg("pid")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.stdout, []);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("ERROR"), "{stderr}");
    assert!(stderr.contains("command failed"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr}");
    assert!(!stderr.contains("DEBUG"), "{stderr}");
}

#[test]
fn verbose_includes_debug_diagnostics_without_polluting_stdout() {
    let output = Command::new(env!("CARGO_BIN_EXE_kurumi-containerd"))
        .env("HOME", "/dev/null")
        .args(["--verbose", "pid"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.stdout, []);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("DEBUG"), "{stderr}");
    assert!(stderr.contains("ERROR"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr}");
}
