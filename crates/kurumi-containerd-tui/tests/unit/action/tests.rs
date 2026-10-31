// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn install_uses_selected_management_name_and_exact_arguments() {
    let action = Action::Install {
        archive: "rootfs.tar".into(),
        size: Some("8G".into()),
        force: true,
    };
    assert_eq!(
        action.args("debian"),
        vec![
            "--name",
            "debian",
            "install",
            "rootfs.tar",
            "--size",
            "8G",
            "--force"
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>()
    );
}

#[test]
fn host_check_does_not_require_a_container() {
    assert_eq!(Action::Check.args("debian"), [OsString::from("check")]);
}

#[test]
fn configured_foreground_start_uses_separate_terminal() {
    assert!(Action::Start(false).needs_terminal(true));
    assert!(!Action::Start(false).needs_terminal(false));
}

#[test]
fn backup_requires_successful_inactive_info() {
    let mut entry = Entry {
        pointer: kurumi_containerd_config::ConfigPointer {
            name: "test".into(),
            file: "container.toml".into(),
        },
        info: Ok(kurumi_containerd_runtime::ContainerInfo {
            name: "runtime-name".into(),
            active: false,
            init_pid: None,
            monitor_pid: None,
            rootfs: "/rootfs".into(),
            uuid: None,
            init_system: None,
            generation: None,
            uptime_seconds: None,
            memory_kb: None,
            processes: None,
        }),
        foreground: true,
        install_ready: true,
    };
    assert!(allowed(ActionKind::Backup, &entry, false).is_ok());
    entry.info.as_mut().unwrap().active = true;
    assert!(allowed(ActionKind::Backup, &entry, false).is_err());
    entry.info = Err(anyhow::anyhow!("rootfs unavailable"));
    assert!(allowed(ActionKind::Backup, &entry, false).is_err());
}
