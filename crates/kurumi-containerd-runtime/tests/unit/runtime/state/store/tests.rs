// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::sync::mpsc;
use std::time::Duration;

use super::*;

fn fixture() -> ContainerState {
    serde_json::from_value(serde_json::json!({
        "name": "demo", "init_pid": 1, "monitor_pid": 2,
        "rootfs": "/rootfs", "uuid": "00000000-0000-0000-0000-000000000000",
        "host_boot_id": "boot", "init_start_time": 10,
        "pid_namespace_inode": 20, "started_at_unix": 30, "monitor_start_time": 40
    }))
    .unwrap()
}

#[test]
fn publication_is_shared_and_stale_cleanup_preserves_new_generation() {
    let store = StateStore::new().unwrap();
    let monitor = store.clone();
    let original = fixture();
    store.write_state(&original).unwrap();
    let mut next = original.clone();
    next.generation += 1;
    monitor.write_state(&next).unwrap();
    store.remove_state_for(&original).unwrap();
    assert_eq!(
        read_record(&mut store.0.records.lock().unwrap()).unwrap(),
        Some(next.clone())
    );
    monitor.remove_state_for(&next).unwrap();
    assert!(
        read_record(&mut store.0.records.lock().unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn incomplete_inactive_slot_does_not_replace_published_state() {
    let store = StateStore::new().unwrap();
    let state = fixture();
    store.write_state(&state).unwrap();
    {
        let mut file = store.0.records.lock().unwrap();
        file.seek(std::io::SeekFrom::Start(slot_offset(2))).unwrap();
        file.write_all(b"incomplete update").unwrap();
    }
    assert_eq!(
        read_record(&mut store.0.records.lock().unwrap()).unwrap(),
        Some(state)
    );
}

#[test]
fn lifecycle_lock_and_stop_request_have_independent_lifetimes() {
    let store = StateStore::new().unwrap();
    let lock = store.lock().unwrap();
    assert!(store.try_lock().unwrap().is_none());
    assert!(!store.stop_requested().unwrap());
    let stop = store.request_stop().unwrap();
    assert!(store.stop_requested().unwrap());
    drop(stop);
    assert!(!store.stop_requested().unwrap());
    drop(lock);
    assert!(store.try_lock().unwrap().is_some());
}

#[test]
fn cloned_store_serializes_concurrent_lifecycle_operations() {
    let store = StateStore::new().unwrap();
    let lock = store.lock().unwrap();
    let worker_store = store.clone();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _lock = worker_store.lock().unwrap();
        sender.send(()).unwrap();
    });
    assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    drop(lock);
    receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.join().unwrap();
}

#[cfg(feature = "servicectl")]
#[test]
fn worker_cleanup_checks_generation_and_boot_identity() {
    let store = StateStore::new().unwrap();
    let state = crate::runtime::state::tests::shared_state();
    store.write_state(&state).unwrap();
    assert!(
        !store
            .remove_shared_state_for_worker(
                state.worker_pid.unwrap(),
                state.worker_start_time.unwrap() + 1
            )
            .unwrap()
    );
    assert_eq!(store.state().unwrap(), Some(state.clone()));
    assert!(
        store
            .remove_shared_state_for_worker(
                state.worker_pid.unwrap(),
                state.worker_start_time.unwrap()
            )
            .unwrap()
    );
    assert!(store.state().unwrap().is_none());
}
