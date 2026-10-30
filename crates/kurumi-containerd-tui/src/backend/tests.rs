// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

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

#[test]
fn captures_exit_status_for_success_and_failure() {
    for (exe, code) in [("/bin/true", 0), ("/bin/false", 1)] {
        let Launch::Captured(mut task) = Backend::new(Path::new(exe))
            .launch(&Action::Check, None)
            .unwrap()
        else {
            panic!("expected captured command")
        };
        assert_eq!(complete(&mut task), format!("Exit: exit status: {code}\n"));
    }
}

#[test]
fn captures_both_streams_lossily_and_bounds_large_output() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("command");
    fs::write(&exe, "#!/bin/sh\nprintf 'stdout\\n'\nprintf 'stderr\\n' >&2\nprintf '\\377'\nhead -c 1048576 /dev/zero\nprintf 'not captured'\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let Launch::Captured(mut task) = Backend::new(&exe).launch(&Action::Check, None).unwrap()
    else {
        panic!("expected captured command")
    };
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
    let exe = dir.path().join("command");
    let release = dir.path().join("release");
    fs::write(
        &exe,
        format!(
            "#!/bin/sh\nwhile [ ! -f '{}' ]; do sleep 0.01; done\nprintf done\n",
            release.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let Launch::Captured(mut task) = Backend::new(&exe).launch(&Action::Check, None).unwrap()
    else {
        panic!("expected captured command")
    };
    assert!(task.poll().unwrap().is_none());
    fs::write(release, "").unwrap();
    assert_eq!(complete(&mut task), "Exit: exit status: 0\ndone");
}

#[test]
fn invalid_and_install_only_entries_preserve_source_and_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("container.toml");
    fs::write(&file, "invalid = [").unwrap();
    let mut entries = [load_entry(ConfigPointer {
        name: "display".into(),
        file: file.clone(),
    })];
    assert!(entries[0].info.is_err());
    assert!(!entries[0].install_ready);
    assert_eq!(fs::read_to_string(&file).unwrap(), "invalid = [");

    let source = "[runtime]\n[container]\nname = 'tui-decoupling-test'\nrootfs = 'missing-rootfs'\nforeground = true\n";
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
