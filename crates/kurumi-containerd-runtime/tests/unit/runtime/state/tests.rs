// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

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
    assert_eq!(state.isolation, IsolationMode::Isolated);
    assert_eq!(state.pid_namespace_inode, Some(2));
    assert_eq!(state.worker_pid, None);
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
        pid_namespace_inode: Some(2),
        started_at_unix: 3,
        monitor_start_time: 4,
        init_system: crate::InitSystem::Unknown,
        generation: 0,
        isolation: IsolationMode::Isolated,
        worker_pid: None,
        worker_start_time: None,
        mount_namespace_inode: None,
        cgroup_path: None,
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

#[cfg(feature = "servicectl")]
pub(super) fn shared_state() -> ContainerState {
    ContainerState {
        name: "test".into(),
        init_pid: 2_000_000_001,
        monitor_pid: 2_000_000_002,
        rootfs: "/rootfs".into(),
        uuid: Uuid::new_v4(),
        host_boot_id: host_boot_id().unwrap(),
        init_start_time: 30,
        pid_namespace_inode: None,
        started_at_unix: 0,
        monitor_start_time: 10,
        init_system: InitSystem::Unknown,
        generation: 3,
        isolation: IsolationMode::SharedPidIpc,
        worker_pid: Some(2_000_000_003),
        worker_start_time: Some(20),
        mount_namespace_inode: Some(123),
        cgroup_path: Some("/delegated/kurumi-containerd/test".into()),
    }
}

#[test]
#[cfg(feature = "servicectl")]
fn shared_records_require_complete_identity_but_no_pid_namespace() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.json");
    let state = shared_state();
    write_state_atomic(&path, &state).unwrap();
    assert_eq!(read_state(&path).unwrap(), state);
    for field in [
        "worker_pid",
        "worker_start_time",
        "mount_namespace_inode",
        "cgroup_path",
    ] {
        let mut value = serde_json::to_value(&state).unwrap();
        value.as_object_mut().unwrap().remove(field);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_state(&path).is_err(), "accepted missing {field}");
    }
}

#[test]
#[cfg(feature = "servicectl")]
fn shared_residual_state_is_retained_without_live_supervision() {
    let state = shared_state();
    assert!(state_is_present(&state));
    assert!(!validate_process_identity(&state));
    assert!(shared_cgroup_directory(&state).is_err());
    let mut old_boot = state;
    old_boot.host_boot_id = "previous boot".into();
    assert!(!state_is_present(&old_boot));
}

#[test]
#[cfg(feature = "servicectl")]
fn shared_identity_rejects_other_container_and_unsafe_cgroup_paths() {
    let mut state = shared_state();
    for path in [
        "/delegated/kurumi-containerd/other",
        "/delegated/kurumi-containerd/test/child",
        "/delegated/kurumi-containerd/../test",
        "/delegated//kurumi-containerd/test",
        "kurumi-containerd/test",
    ] {
        state.cgroup_path = Some(path.into());
        assert!(!shared_metadata_valid(&state), "accepted {path}");
    }
}

#[test]
#[cfg(feature = "servicectl")]
fn shared_membership_matches_only_recorded_unified_subtree() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join(std::process::id().to_string());
    fs::create_dir(&root).unwrap();
    let process = Process::new_with_root(root.clone()).unwrap();
    let state = shared_state();
    for membership in [
        "0::/delegated/kurumi-containerd/test\n",
        "0::/delegated/kurumi-containerd/test/services/nested\n",
    ] {
        fs::write(root.join("cgroup"), membership).unwrap();
        assert!(shared_membership_matches(&state, &process));
    }
    for membership in [
        "0::/delegated/kurumi-containerd/test-other\n",
        "0::/delegated/kurumi-containerd/other\n",
        "2:memory:/delegated/kurumi-containerd/test\n",
    ] {
        fs::write(root.join("cgroup"), membership).unwrap();
        assert!(!shared_membership_matches(&state, &process));
    }
}

#[test]
#[cfg(feature = "servicectl")]
fn shared_identity_uses_recorded_ancestry_mount_and_markers_without_nspid() {
    use std::os::unix::fs::MetadataExt;
    let current = Process::myself().unwrap();
    let stat = current.stat().unwrap();
    let worker = Process::new(stat.ppid).unwrap();
    let worker_stat = worker.stat().unwrap();
    let monitor = Process::new(worker_stat.ppid).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join(current.pid.to_string());
    fs::create_dir_all(root.join("root/run/kurumi-containerd")).unwrap();
    fs::create_dir(root.join("ns")).unwrap();
    fs::write(root.join("ns/mnt"), "private mount").unwrap();
    fs::write(
        root.join("stat"),
        fs::read_to_string("/proc/self/stat").unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("cgroup"),
        "0::/delegated/kurumi-containerd/test/kurumi-init\n",
    )
    .unwrap();
    let mut state = shared_state();
    state.init_pid = current.pid;
    state.init_start_time = stat.starttime;
    state.worker_pid = Some(worker.pid);
    state.worker_start_time = Some(worker_stat.starttime);
    state.monitor_pid = monitor.pid;
    state.monitor_start_time = monitor.stat().unwrap().starttime;
    state.mount_namespace_inode = Some(fs::metadata(root.join("ns/mnt")).unwrap().ino());
    fs::write(root.join("root/run/kurumi-containerd/name"), &state.name).unwrap();
    fs::write(
        root.join("root/run/kurumi-containerd/uuid"),
        state.uuid.to_string(),
    )
    .unwrap();
    let process = Process::new_with_root(root.clone()).unwrap();
    assert!(validate_process_identity_for(&state, &process));
    for field in [
        "init_start_time",
        "worker_start_time",
        "mount_namespace_inode",
    ] {
        let mut wrong = serde_json::to_value(&state).unwrap();
        wrong[field] = (wrong[field].as_u64().unwrap() + 1).into();
        let wrong = serde_json::from_value(wrong).unwrap();
        assert!(
            !validate_process_identity_for(&wrong, &process),
            "accepted wrong {field}"
        );
    }
    fs::write(
        root.join("root/run/kurumi-containerd/uuid"),
        Uuid::new_v4().to_string(),
    )
    .unwrap();
    assert!(!validate_process_identity_for(&state, &process));
}
