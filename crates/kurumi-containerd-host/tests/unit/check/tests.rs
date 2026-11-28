// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn report_represents_available_and_unavailable_capabilities() {
    let report = HostCheckReport {
        capabilities: vec![
            capability("available", true, "working"),
            capability("missing", false, "not installed"),
        ],
    };

    assert!(report.capabilities[0].available);
    assert!(!report.capabilities[1].available);
}

#[test]
fn base_check_does_not_require_pid_ipc_or_network_namespaces() {
    let mut report = HostCheckReport {
        capabilities: vec![
            capability("Mount namespace", true, ""),
            capability("UTS namespace", true, ""),
            capability("PID namespace", false, ""),
            capability("IPC namespace", false, ""),
            capability("Network namespace", false, ""),
            capability("Cgroup namespace", false, ""),
        ],
    };
    assert!(report.required_namespaces_available());
    report.capabilities[0].available = false;
    assert!(!report.required_namespaces_available());
}

#[test]
fn filesystem_checks_match_registered_types_exactly() {
    let filesystems = "nodev\tproc\nnodev\tdevtmpfs\nnodev\toverlayfs\n\text4\n";
    let reported = filesystem_capabilities(filesystems)
        .map(|entry| (entry.name, entry.available))
        .collect::<std::collections::HashMap<_, _>>();

    assert!(reported["procfs"]);
    assert!(reported["ext4"]);
    assert!(!reported["tmpfs"]);
    assert!(!reported["OverlayFS"]);
    assert!(!reported["binfmt_misc"]);
}

#[test]
fn controllers_report_the_visible_hierarchy_and_match_whole_names() {
    let mounts =
        [MountInfo::from_line("1 2 0:1 / /cpu rw - cgroup cgroup rw,cpu,cpuacct").unwrap()];
    let [cpu, memory, pids] = controller_capabilities(&mounts, Some("memory pids_custom\n"));
    assert!(cpu.available && cpu.detail.contains("v1"));
    assert!(memory.available && memory.detail.contains("v2"));
    assert!(!pids.available);

    assert!(
        controller_capabilities(&[], None)
            .iter()
            .all(|entry| !entry.available)
    );
}
