use super::*;

#[test]
fn failed_lease_commit_restores_original_enforcement() {
    for original in ["1", "0"] {
        let dir = tempfile::tempdir().unwrap();
        let enforce = dir.path().join("enforce");
        fs::write(&enforce, original).unwrap();
        fs::create_dir(dir.path().join("selinux-state.json.tmp")).unwrap();
        let config = AndroidConfig {
            selinux_permissive: true,
            ..AndroidConfig::default()
        };
        assert!(SelinuxGuard::apply_at(&config, dir.path(), &enforce).is_err());
        assert_eq!(fs::read_to_string(&enforce).unwrap(), original);
    }
}

#[test]
fn failed_restore_keeps_recovery_state() {
    let dir = tempfile::tempdir().unwrap();
    let enforce = dir.path().join("enforce");
    fs::write(&enforce, "1").unwrap();
    let config = AndroidConfig {
        selinux_permissive: true,
        ..AndroidConfig::default()
    };
    let guard = SelinuxGuard::apply_at(&config, dir.path(), &enforce).unwrap();
    fs::remove_file(&enforce).unwrap();
    fs::create_dir(&enforce).unwrap();
    drop(guard);
    assert!(dir.path().join("selinux-state.json").is_file());
}

#[test]
fn malformed_lease_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let enforce = dir.path().join("enforce");
    fs::write(&enforce, "1").unwrap();
    let state = dir.path().join("selinux-state.json");
    fs::write(&state, "broken").unwrap();
    let config = AndroidConfig {
        selinux_permissive: true,
        ..AndroidConfig::default()
    };
    assert!(SelinuxGuard::apply_at(&config, dir.path(), &enforce).is_err());
    assert_eq!(fs::read_to_string(&state).unwrap(), "broken");
    assert_eq!(fs::read_to_string(&enforce).unwrap(), "1");
}

#[test]
fn disabled_request_does_not_access_selinux() {
    let dir = tempfile::tempdir().unwrap();
    drop(SelinuxGuard::apply(&AndroidConfig::default(), dir.path()).unwrap());
    assert!(!dir.path().join("selinux-state.json").exists());
}

#[test]
fn only_last_lease_restores_original_enforcement() {
    for original in ["1", "0"] {
        let dir = tempfile::tempdir().unwrap();
        let enforce = dir.path().join("enforce");
        fs::write(&enforce, original).unwrap();
        let config = AndroidConfig {
            selinux_permissive: true,
            ..AndroidConfig::default()
        };
        let first = SelinuxGuard::apply_at(&config, dir.path(), &enforce).unwrap();
        let second = SelinuxGuard::apply_at(&config, dir.path(), &enforce).unwrap();
        assert_eq!(fs::read_to_string(&enforce).unwrap(), "0");
        drop(first);
        assert_eq!(fs::read_to_string(&enforce).unwrap(), "0");
        drop(second);
        assert_eq!(fs::read_to_string(&enforce).unwrap(), original);
        assert!(!dir.path().join("selinux-state.json").exists());
    }
}
