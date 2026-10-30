// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::process::Command;

fn diagnostic_output(output: &std::process::Output) -> &[u8] {
    if cfg!(debug_assertions) {
        &output.stdout
    } else {
        &output.stderr
    }
}

fn command_output(output: &std::process::Output) -> &[u8] {
    if cfg!(debug_assertions) {
        &output.stderr
    } else {
        &output.stdout
    }
}

#[test]
fn command_failure_is_a_diagnostic_on_build_selected_stream() {
    let output = Command::new(env!("CARGO_BIN_EXE_kurumi-containerd"))
        .env("HOME", "/dev/null")
        .arg("pid")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(command_output(&output), []);
    let diagnostics = String::from_utf8(diagnostic_output(&output).to_vec()).unwrap();
    assert!(diagnostics.contains("ERROR"), "{diagnostics}");
    assert!(diagnostics.contains("command failed"), "{diagnostics}");
    assert!(!diagnostics.contains('\u{1b}'), "{diagnostics}");
    assert!(!diagnostics.contains("DEBUG"), "{diagnostics}");
}

#[test]
fn verbose_includes_debug_diagnostics_on_build_selected_stream() {
    let output = Command::new(env!("CARGO_BIN_EXE_kurumi-containerd"))
        .env("HOME", "/dev/null")
        .args(["--verbose", "pid"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(command_output(&output), []);
    let diagnostics = String::from_utf8(diagnostic_output(&output).to_vec()).unwrap();
    assert!(diagnostics.contains("DEBUG"), "{diagnostics}");
    assert!(diagnostics.contains("ERROR"), "{diagnostics}");
    assert!(!diagnostics.contains('\u{1b}'), "{diagnostics}");
}
