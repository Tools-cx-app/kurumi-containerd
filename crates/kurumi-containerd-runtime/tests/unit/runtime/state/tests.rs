use super::*;

#[test]
fn old_state_defaults_to_unknown_init() {
    let state: ContainerState = serde_json::from_value(serde_json::json!({
        "name": "test",
        "init_pid": 1,
        "monitor_pid": 2,
        "rootfs": "/rootfs",
        "uuid": "00000000-0000-0000-0000-000000000000",
        "host_boot_id": "boot",
        "init_start_time": 1,
        "pid_namespace_inode": 2,
        "started_at_unix": 3,
        "monitor_start_time": 4
    }))
    .unwrap();
    assert_eq!(state.init_system, crate::InitSystem::Unknown);
    assert_eq!(state.generation, 0);
}

#[test]
fn parses_namespace_and_parent_pids() {
    let pid = i32::try_from(std::process::id()).unwrap();
    assert!(
        namespace_pid(&Process::new(pid).unwrap())
            .unwrap()
            .is_some()
    );
    assert_eq!(parent_pid(pid).unwrap(), current_parent_pid());
}

#[test]
fn writes_state_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.json");
    let state = ContainerState {
        name: "test".to_owned(),
        init_pid: 1,
        monitor_pid: 2,
        rootfs: PathBuf::from("/rootfs"),
        uuid: Uuid::nil(),
        host_boot_id: "boot".to_owned(),
        init_start_time: 1,
        pid_namespace_inode: 2,
        started_at_unix: 3,
        monitor_start_time: 4,
        init_system: crate::InitSystem::Unknown,
        generation: 0,
    };
    write_state_atomic(&path, &state).unwrap();
    assert_eq!(read_state(&path).unwrap().uuid, Uuid::nil());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn rejects_symlink_state_file() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.json");
    fs::write(&target, "{}").unwrap();
    let path = directory.path().join("state.json");
    std::os::unix::fs::symlink(target, &path).unwrap();

    assert!(read_state(&path).is_err());
}
