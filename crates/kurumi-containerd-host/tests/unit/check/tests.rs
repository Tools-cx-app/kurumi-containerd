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
