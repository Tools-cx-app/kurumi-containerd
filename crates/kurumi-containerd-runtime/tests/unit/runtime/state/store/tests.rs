// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::Duration,
};

use tempfile::{TempDir, tempdir};
use uuid::Uuid;

fn fixture() -> (TempDir, StateStore, ContainerState) {
    let directory = tempdir().unwrap();
    let store = StateStore::new(directory.path(), "demo");
    // Unit tests exercise persistence without requiring root-owned directories.
    fs::create_dir(store.state_dir()).unwrap();
    fs::create_dir(store.recovery_dir()).unwrap();
    let state = ContainerState {
        name: "demo".to_owned(),
        init_pid: 10,
        monitor_pid: 9,
        rootfs: PathBuf::from("/rootfs"),
        uuid: Uuid::nil(),
        host_boot_id: "boot".to_owned(),
        init_start_time: 100,
        pid_namespace_inode: 200,
        started_at_unix: 300,
        monitor_start_time: 90,
        init_system: crate::InitSystem::Unknown,
        generation: 0,
    };
    store.write_state(&state).unwrap();
    (directory, store, state)
}

#[test]
fn selects_state_and_recovery_paths_from_workdir_and_name() {
    let store = StateStore::new(Path::new("/run/kurumi"), "demo");
    assert_eq!(store.state_dir(), Path::new("/run/kurumi/state"));
    assert_eq!(store.recovery_dir(), Path::new("/run/kurumi/recovery"));
    assert_eq!(store.state_path(), Path::new("/run/kurumi/state/demo.json"));
}

#[test]
fn delayed_recovery_cannot_overwrite_restart_or_reboot_state() {
    for reboot in [false, true] {
        let (directory, store, original) = fixture();
        let workdir = directory.path().to_path_buf();
        let (snapshot_sender, snapshot_receiver) = mpsc::channel();
        let (commit_sender, commit_receiver) = mpsc::channel();
        let scan = thread::spawn(move || {
            let store = StateStore::new(&workdir, "demo");
            let snapshot = read_state(&store.recovery_path(Uuid::nil())).unwrap();
            let mut recovered = snapshot.clone();
            recovered.init_pid += 1;
            snapshot_sender.send(()).unwrap();
            commit_receiver.recv().unwrap();
            store
                .restore_state_if_unchanged(&snapshot, &recovered)
                .unwrap()
        });
        snapshot_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let mut newer = original;
        newer.init_pid += 2;
        newer.init_start_time += 2;
        newer.pid_namespace_inode += 2;
        if reboot {
            newer.generation += 1;
        } else {
            newer.monitor_pid += 2;
            newer.monitor_start_time += 2;
        }
        store.write_state(&newer).unwrap();
        commit_sender.send(()).unwrap();

        assert!(!scan.join().unwrap());
        assert_eq!(read_state(&store.state_path()).unwrap(), newer);
        assert_eq!(read_state(&store.recovery_path(newer.uuid)).unwrap(), newer);
    }
}

#[test]
fn recovery_does_not_resurrect_removed_records() {
    let (_directory, store, original) = fixture();
    store.remove_state_for(&original).unwrap();

    assert!(
        !store
            .restore_state_if_unchanged(&original, &original)
            .unwrap()
    );
    assert!(!store.state_path().exists());
    assert!(!store.recovery_path(original.uuid).exists());
}

#[test]
fn recovery_preserves_new_identity_even_if_old_recovery_record_remains() {
    let (_directory, store, original) = fixture();
    let mut newer = original.clone();
    newer.uuid = Uuid::from_u128(1);
    newer.monitor_pid += 1;
    newer.monitor_start_time += 1;
    store.write_state(&newer).unwrap();

    assert!(
        !store
            .restore_state_if_unchanged(&original, &original)
            .unwrap()
    );
    assert_eq!(read_state(&store.state_path()).unwrap(), newer);
    assert_eq!(read_state(&store.recovery_path(newer.uuid)).unwrap(), newer);
}

#[test]
fn stale_recovery_cleanup_preserves_replaced_record() {
    let (_directory, store, original) = fixture();
    let mut newer = original.clone();
    newer.monitor_pid += 1;
    newer.monitor_start_time += 1;
    store.write_state(&newer).unwrap();

    store.remove_recovery_if_unchanged(&original).unwrap();

    assert_eq!(read_state(&store.recovery_path(newer.uuid)).unwrap(), newer);
    assert_eq!(read_state(&store.state_path()).unwrap(), newer);
    store.remove_recovery_if_unchanged(&newer).unwrap();
    assert!(!store.recovery_path(newer.uuid).exists());
    assert_eq!(read_state(&store.state_path()).unwrap(), newer);
}

#[test]
fn missing_state_is_restored_while_lifecycle_lock_is_held() {
    let (directory, store, original) = fixture();
    store.remove_state().unwrap();
    let lifecycle_lock = store.lock().unwrap();
    let workdir = directory.path().to_path_buf();
    let snapshot = original.clone();
    let (sender, receiver) = mpsc::channel();
    let scan = thread::spawn(move || {
        let store = StateStore::new(&workdir, "demo");
        sender
            .send(
                store
                    .restore_state_if_unchanged(&snapshot, &snapshot)
                    .unwrap(),
            )
            .unwrap();
    });
    let result = receiver.recv_timeout(Duration::from_secs(2));
    // Release even on timeout so a mistaken lifecycle lock cannot hang the test.
    drop(lifecycle_lock);
    scan.join().unwrap();

    assert!(result.expect("recovery re-entered the lifecycle lock"));
    assert_eq!(read_state(&store.state_path()).unwrap(), original);
}

#[test]
fn state_changes_serialize_with_recovery_commits() {
    let operations: [fn(&StateStore, &ContainerState); 5] = [
        |store, state| store.write_state(state).unwrap(),
        |store, _| store.remove_state().unwrap(),
        |store, state| store.remove_state_for(state).unwrap(),
        |store, state| store.remove_recovery_if_unchanged(state).unwrap(),
        |store, state| {
            assert!(store.restore_state_if_unchanged(state, state).unwrap());
        },
    ];
    for operation in operations {
        let (directory, store, state) = fixture();
        let lock = store.lock_updates(&state.name).unwrap();
        let workdir = directory.path().to_path_buf();
        let (started_sender, started_receiver) = mpsc::channel();
        let (finished_sender, finished_receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let store = StateStore::new(&workdir, "demo");
            started_sender.send(()).unwrap();
            operation(&store, &state);
            finished_sender.send(()).unwrap();
        });
        started_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let result = finished_receiver.recv_timeout(Duration::from_millis(50));
        drop(lock);
        worker.join().unwrap();

        assert_eq!(result, Err(RecvTimeoutError::Timeout));
    }
}

#[test]
fn state_update_lock_rejects_symlinks() {
    let (directory, store, state) = fixture();
    let target = directory.path().join("untouched");
    fs::write(&target, "keep").unwrap();
    let lock_path = store.state_dir().join("demo.lock");
    fs::remove_file(&lock_path).unwrap();
    std::os::unix::fs::symlink(&target, lock_path).unwrap();

    assert!(store.write_state(&state).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "keep");
}

#[test]
fn stop_request_is_distinct_from_lifecycle_contention_and_expires_on_release() {
    let (_directory, store, _state) = fixture();
    let _lifecycle = store.lock().unwrap();
    assert!(!store.stop_requested().unwrap());
    let request = store.request_stop().unwrap();
    assert!(store.stop_requested().unwrap());
    drop(request);
    assert!(!store.stop_requested().unwrap());
}

#[test]
fn stop_request_lock_rejects_symlinks() {
    let (directory, store, _state) = fixture();
    let target = directory.path().join("untouched");
    fs::write(&target, "keep").unwrap();
    std::os::unix::fs::symlink(&target, store.state_dir().join("demo.stop")).unwrap();
    assert!(store.request_stop().is_err());
    assert!(store.stop_requested().is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "keep");
}
