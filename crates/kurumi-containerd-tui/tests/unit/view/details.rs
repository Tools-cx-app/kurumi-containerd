// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use kurumi_containerd_runtime::ContainerInfo;
use uuid::Uuid;

use super::*;

#[test]
fn details_preserve_running_and_inactive_container_fields() {
    let mut info = ContainerInfo {
        name: "test".into(),
        active: true,
        isolation: kurumi_containerd_config::IsolationMode::Isolated,
        init_pid: Some(123),
        monitor_pid: Some(122),
        rootfs: "/rootfs".into(),
        uuid: Some(Uuid::nil()),
        init_system: Some(kurumi_containerd_runtime::InitSystem::Systemd),
        generation: Some(1),
        uptime_seconds: Some(93_784),
        memory_kb: Some(1_572_864),
        processes: Some(4),
    };
    assert_eq!(
        info_lines(&info).join("\n"),
        "Name: test\nIsolation: isolated\nActive: active (running) for 1d 02h 03m 04s\nMain PID: 123 (systemd)\nMonitor PID: 122\nTasks: 4\nMemory: 1.5 GiB\nGeneration: 1\nRootfs: /rootfs\nUUID: 00000000-0000-0000-0000-000000000000"
    );
    info.init_pid = None;
    assert!(
        info_lines(&info)
            .iter()
            .any(|line| line.contains("active (cleanup pending)"))
    );
    info.active = false;
    #[cfg(feature = "servicectl")]
    {
        info.isolation = kurumi_containerd_config::IsolationMode::SharedPidIpc;
    }
    info.uuid = None;
    assert_eq!(
        info_lines(&info).join("\n"),
        format!(
            "Name: test\nIsolation: {}\nActive: inactive (dead)\nRootfs: /rootfs\nUUID: unassigned",
            info.isolation
        )
    );
}

#[test]
fn detail_units_cover_short_durations_and_small_memory() {
    assert_eq!(format_duration(0), "0s");
    assert_eq!(format_duration(62), "1m 02s");
    assert_eq!(format_duration(3_723), "1h 02m 03s");
    assert_eq!(format_memory(512), "512 KiB");
    assert_eq!(format_memory(1_536), "1.5 MiB");
}
