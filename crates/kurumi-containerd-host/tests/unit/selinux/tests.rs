// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn only_last_owner_restores_original_enforcement_without_state_files() {
    for original in ["1", "0"] {
        let dir = tempfile::tempdir().unwrap();
        let enforce = dir.path().join("enforce");
        fs::write(&enforce, original).unwrap();
        let config = AndroidConfig {
            selinux_permissive: true,
            ..AndroidConfig::default()
        };
        let state = SelinuxState::new().unwrap();
        let first = SelinuxGuard::apply_at(&config, &state, &enforce).unwrap();
        let second = SelinuxGuard::apply_at(&config, &state, &enforce).unwrap();
        assert_eq!(fs::read_to_string(&enforce).unwrap(), "0");
        drop(first);
        assert_eq!(fs::read_to_string(&enforce).unwrap(), "0");
        drop(second);
        assert_eq!(fs::read_to_string(&enforce).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn failed_restore_preserves_memory_ownership_for_retry() {
    let dir = tempfile::tempdir().unwrap();
    let enforce = dir.path().join("enforce");
    fs::write(&enforce, "1").unwrap();
    let state = SelinuxState::new().unwrap();
    let config = AndroidConfig {
        selinux_permissive: true,
        ..AndroidConfig::default()
    };
    let guard = SelinuxGuard::apply_at(&config, &state, &enforce).unwrap();
    fs::remove_file(&enforce).unwrap();
    fs::create_dir(&enforce).unwrap();
    assert!(guard.release(&state).is_err());
    fs::remove_dir(&enforce).unwrap();
    fs::write(&enforce, "0").unwrap();
    drop(guard);
    assert_eq!(fs::read_to_string(&enforce).unwrap(), "1");
}

#[test]
fn disabled_request_does_not_access_selinux() {
    let state = SelinuxState::new().unwrap();
    drop(
        SelinuxGuard::apply_at(
            &AndroidConfig::default(),
            &state,
            Path::new("/nonexistent/selinux"),
        )
        .unwrap(),
    );
}
