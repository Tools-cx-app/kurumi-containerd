use super::*;

#[test]
fn formats_uptime() {
    assert_eq!(format_duration(0), "0s");
    assert_eq!(format_duration(62), "1m 02s");
    assert_eq!(format_duration(3_723), "1h 02m 03s");
    assert_eq!(format_duration(93_784), "1d 02h 03m 04s");
}

#[test]
fn formats_memory() {
    assert_eq!(format_memory(512), "512 KiB");
    assert_eq!(format_memory(1_536), "1.5 MiB");
    assert_eq!(format_memory(1_572_864), "1.5 GiB");
}

#[test]
fn displays_container_info() {
    let info = ContainerInfo {
        name: "test".to_owned(),
        active: true,
        init_pid: Some(123),
        monitor_pid: Some(122),
        rootfs: PathBuf::from("/rootfs"),
        uuid: Some(Uuid::nil()),
        init_system: Some(crate::InitSystem::Systemd),
        generation: Some(1),
        uptime_seconds: Some(3_723),
        memory_kb: Some(1_536),
        processes: Some(4),
    };
    assert_eq!(
        info.to_string(),
        "Name: test\nActive: active (running) for 1h 02m 03s\nMain PID: 123 (systemd)\nMonitor PID: 122\nTasks: 4\nMemory: 1.5 MiB\nGeneration: 1\nRootfs: /rootfs\nUUID: 00000000-0000-0000-0000-000000000000"
    );
}

#[test]
fn displays_inactive_container_info() {
    let info = ContainerInfo {
        name: "test".to_owned(),
        active: false,
        init_pid: None,
        monitor_pid: None,
        rootfs: PathBuf::from("/rootfs"),
        uuid: Some(Uuid::nil()),
        init_system: None,
        generation: None,
        uptime_seconds: None,
        memory_kb: None,
        processes: None,
    };
    assert_eq!(
        info.to_string(),
        "Name: test\nActive: inactive (dead)\nRootfs: /rootfs\nUUID: 00000000-0000-0000-0000-000000000000"
    );
}

#[test]
fn displays_container_info_lines() {
    let info = ContainerInfo {
        name: "test".to_owned(),
        active: true,
        init_pid: Some(123),
        monitor_pid: Some(122),
        rootfs: PathBuf::from("/a/very/long/rootfs/path"),
        uuid: Some(Uuid::nil()),
        init_system: Some(crate::InitSystem::Systemd),
        generation: Some(1),
        uptime_seconds: Some(3_723),
        memory_kb: Some(1_536),
        processes: Some(4),
    };
    let lines = info.display_lines();
    assert!(
        lines
            .iter()
            .any(|line| line == "Rootfs: /a/very/long/rootfs/path")
    );
    assert!(lines.iter().any(|line| line.starts_with("UUID: ")));

    let inactive = ContainerInfo {
        active: false,
        init_pid: None,
        monitor_pid: None,
        init_system: None,
        generation: None,
        uptime_seconds: None,
        memory_kb: None,
        processes: None,
        ..info
    };
    assert!(
        inactive
            .display_lines()
            .iter()
            .any(|line| line == "Active: inactive (dead)")
    );
}

#[test]
fn reports_inactive_container_without_state() {
    let source = "[runtime]\n\n[container]\nname = \"test\"\nrootfs = \"/rootfs\"\n";
    let config: kurumi_containerd_config::Config = toml::from_str(source).unwrap();
    let info = Runtime::new(config).unwrap().info().unwrap();
    assert!(!info.active);
    assert_eq!(info.name, "test");
    assert_eq!(info.rootfs, PathBuf::from("/rootfs"));
    assert_eq!(info.init_pid, None);
}
