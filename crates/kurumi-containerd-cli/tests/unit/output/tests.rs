// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn container_info_output_preserves_running_and_inactive_details() {
    let mut container = ContainerInfo {
        name: "test".into(),
        active: true,
        init_pid: Some(123),
        monitor_pid: Some(122),
        rootfs: "/rootfs".into(),
        uuid: None,
        init_system: Some(kurumi_containerd_runtime::InitSystem::Systemd),
        generation: Some(1),
        uptime_seconds: Some(3_723),
        memory_kb: Some(1_536),
        processes: Some(4),
    };
    let mut out = Vec::new();
    info(&mut out, &container).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "Name: test\nActive: active (running) for 1h 02m 03s\nMain PID: 123 (systemd)\nMonitor PID: 122\nTasks: 4\nMemory: 1.5 MiB\nGeneration: 1\nRootfs: /rootfs\nUUID: unassigned\n"
    );
    container.active = false;
    let mut out = Vec::new();
    info(&mut out, &container).unwrap();
    assert_eq!(
        out,
        b"Name: test\nActive: inactive (dead)\nRootfs: /rootfs\nUUID: unassigned\n"
    );
    let mut full = &mut [][..];
    assert!(info(&mut full, &container).is_err());
}

#[test]
fn info_units_cover_duration_and_memory_boundaries() {
    assert_eq!(format_duration(0), "0s");
    assert_eq!(format_duration(62), "1m 02s");
    assert_eq!(format_duration(93_784), "1d 02h 03m 04s");
    assert_eq!(format_memory(512), "512 KiB");
    assert_eq!(format_memory(1_572_864), "1.5 GiB");
}

#[test]
fn pid_is_numeric_and_foreground_reports_completion() {
    let mut out = Vec::new();
    pid(&mut out, 1234).unwrap();
    assert_eq!(out, b"1234\n");
    out.clear();
    started(&mut out, "demo", 1234, true).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("demo"));
    assert!(text.contains("finished"));
    assert!(!text.contains("started"));
}

#[test]
fn empty_results_are_explicit_and_writes_are_fallible() {
    let mut out = Vec::new();
    containers(&mut out, &[]).unwrap();
    recovered(&mut out, &[]).unwrap();
    check(&mut out, "Namespaces", false, "permission denied").unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("Running containers: 0"));
    assert!(text.contains("No containers required recovery"));
    assert!(text.contains("unavailable (permission denied)"));
    assert!(!text.contains("INFO"));
    let mut full = &mut [][..];
    let error = pid(&mut full, 1234).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WriteZero);
}

#[test]
fn only_output_broken_pipes_are_ignored() {
    let error = write(|_| Err(io::ErrorKind::BrokenPipe.into())).unwrap_err();
    assert!(error.is_broken_pipe());
    let error = write(|_| Err(io::ErrorKind::PermissionDenied.into())).unwrap_err();
    assert!(!error.is_broken_pipe());
}
