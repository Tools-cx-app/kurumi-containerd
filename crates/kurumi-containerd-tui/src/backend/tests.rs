// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;
use std::{
    fs,
    time::{Duration, Instant},
};

const COMMAND_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/backend-command.sh"
);

fn complete(task: &mut CommandTask) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(output) = task.poll().unwrap() {
            return output;
        }
        assert!(Instant::now() < deadline, "command did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn captured(exe: &Path, action: &Action) -> CommandTask {
    match Backend::new(exe).launch(action, None).unwrap() {
        Launch::Captured(task) => task,
        Launch::Failed(error) => panic!("expected captured command: {error}"),
        Launch::Terminal => panic!("expected captured command, got terminal"),
    }
}

#[test]
fn captures_exit_status_for_success_and_failure() {
    for (exe, code) in [("/bin/true", 0), ("/bin/false", 1)] {
        let mut task = captured(Path::new(exe), &Action::Check);
        assert_eq!(complete(&mut task), format!("Exit: exit status: {code}\n"));
    }
}

#[test]
fn captures_both_streams_lossily_and_bounds_large_output() {
    let mut task = captured(Path::new(COMMAND_FIXTURE), &Action::Check);
    let result = complete(&mut task);
    let (status, body) = result.split_once('\n').unwrap();
    assert_eq!(status, "Exit: exit status: 0");
    assert!(body.starts_with("stdout\nstderr\n\u{fffd}"));
    // One invalid byte expands to the three-byte replacement character.
    assert_eq!(body.len(), 1024 * 1024 + 2);
    assert!(!body.contains("not captured"));
}

#[test]
fn polling_running_command_does_not_block() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let mut task = captured(
        Path::new(COMMAND_FIXTURE),
        &Action::Backup {
            output: release.clone(),
        },
    );
    assert!(task.poll().unwrap().is_none());
    fs::write(release, "").unwrap();
    assert_eq!(complete(&mut task), "Exit: exit status: 0\ndone");
}

#[test]
fn invalid_and_install_only_entries_preserve_source_and_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, "invalid = [").unwrap();
    let mut entries = [load_entry(ConfigEntry {
        name: "tui-decoupling-test".into(),
        path: file.clone(),
    })];
    assert!(entries[0].info.is_err());
    assert!(!entries[0].install_ready);
    assert_eq!(fs::read_to_string(&file).unwrap(), "invalid = [");

    let source = r#"[{"runtime": {}, "container": {"name": "tui-decoupling-test", "rootfs": "missing-rootfs", "foreground": true}}]"#;
    fs::write(&file, source).unwrap();
    let backend = Backend::new(Path::new("unused"));
    backend.refresh(&mut entries);
    assert!(entries[0].info.is_err());
    assert!(entries[0].install_ready);
    assert_eq!(fs::read_to_string(&file).unwrap(), source);

    fs::create_dir_all(dir.path().join("missing-rootfs/sbin")).unwrap();
    fs::write(dir.path().join("missing-rootfs/sbin/init"), "").unwrap();
    backend.refresh(&mut entries);
    let info = entries[0].info.as_ref().unwrap();
    assert_eq!(info.name, "tui-decoupling-test");
    assert!(!info.active);
    assert!(entries[0].foreground);
    assert!(info.uuid.is_none());
    assert_eq!(fs::read_to_string(&file).unwrap(), source);

    fs::write(&file, "invalid = [").unwrap();
    backend.refresh(&mut entries);
    assert!(entries[0].info.is_err());
    assert!(!entries[0].install_ready);
    assert!(entries[0].foreground);
}
