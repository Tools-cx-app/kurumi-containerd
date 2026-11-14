// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;
use serde_json::json;

#[test]
fn selects_by_container_name_and_rejects_invalid_registries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let entries = json!([
        {"runtime": {}, "container": {"name": "one", "rootfs": "one"}},
        {"runtime": {}, "container": {"name": "two", "rootfs": "two"}}
    ]);
    fs::write(&path, entries.to_string()).unwrap();
    let entries = ConfigEntry::load(&path).unwrap();
    assert!(ConfigEntry::select(&entries, None).is_err());
    assert!(ConfigEntry::select(&entries, Some("Two")).is_err());
    let config = ConfigEntry::select(&entries, Some("two"))
        .unwrap()
        .load_for_install()
        .unwrap();
    assert_eq!(config.container.name, "two");
    assert_eq!(config.container.hostname, "two");
    assert_eq!(config.container.rootfs, Some(dir.path().join("two")));

    for source in [
        "[]",
        "{}",
        "null",
        "[null]",
        r#"[{"name":"old","file":"old.toml"}]"#,
        r#"[{"container":{"name":"bad/name"}}]"#,
        r#"[{"container":{"name":""}}]"#,
        r#"[{"container":{"name":".."}}]"#,
        r#"[{"container":{"name":"one"}},{"container":{"name":"one"}}]"#,
    ] {
        fs::write(&path, source).unwrap();
        assert!(ConfigEntry::load(&path).is_err(), "{source}");
    }
}

#[test]
fn rejects_unknown_fields_without_breaking_other_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let mut document = json!([
        {"runtime": {}, "container": {"name": "good", "rootfs": "rootfs"}},
        {"runtime": {}, "container": {"name": "bad", "rootfs": "rootfs"}}
    ]);
    for field in ["file", "name", "extra"] {
        document[1][field] = json!("unsupported");
        fs::write(&path, document.to_string()).unwrap();
        let entries = ConfigEntry::load(&path).unwrap();
        assert!(entries[0].load_for_install().is_ok());
        assert!(entries[1].load_for_install().is_err());
    }
}

#[test]
fn concurrent_writes_preserve_other_entries_and_relative_source_values() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("rootfs/sbin")).unwrap();
    fs::write(dir.path().join("rootfs/sbin/init"), "init").unwrap();
    fs::write(dir.path().join("container.env"), "FILE_ONLY=yes").unwrap();
    let path = dir.path().join("config.json");
    let document = json!([
        {"runtime": {}, "container": {"name": "one", "rootfs": "rootfs", "environment_file": "container.env"}},
        {"runtime": {}, "container": {"name": "two", "rootfs": "rootfs"}},
        {"container": {"name": "invalid", "unknown": true}}
    ]);
    fs::write(&path, document.to_string()).unwrap();
    let entries = ConfigEntry::load(&path).unwrap();
    let handles: Vec<_> = entries
        .into_iter()
        .take(2)
        .map(|entry| {
            std::thread::spawn(move || {
                let first = entry.load_persistent().unwrap();
                assert_eq!(
                    first.container.uuid,
                    entry.load_persistent().unwrap().container.uuid
                );
                first.container.uuid.unwrap()
            })
        })
        .collect();
    let uuids: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_ne!(uuids[0], uuids[1]);
    let persisted = read_document(&path).unwrap();
    assert_eq!(persisted[2], document[2]);
    assert_eq!(persisted[0]["container"]["rootfs"], "rootfs");
    assert!(persisted[0]["container"].get("environment").is_none());
    assert_eq!(persisted[0]["container"]["uuid"], uuids[0].to_string());
    assert_eq!(persisted[1]["container"]["uuid"], uuids[1].to_string());
}

#[test]
fn requires_absolute_home() {
    assert!(path_from_home(None).is_err());
    assert!(path_from_home(Some("".into())).is_err());
    assert!(path_from_home(Some("relative".into())).is_err());
    assert_eq!(
        path_from_home(Some("/home/user".into())).unwrap(),
        Path::new("/home/user/.kurumi-containerd/config.json")
    );
}

#[test]
fn install_creates_missing_registry_and_reuses_the_sole_entry() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".kurumi-containerd/config.json");
    let entry = ConfigEntry::prepare_install(&path, None, false).unwrap();
    let config = entry.load_for_install().unwrap();
    assert_eq!(config.container.name, "default");
    assert_eq!(
        config.container.rootfs,
        Some(path.parent().unwrap().join("default.rootfs"))
    );
    assert!(config.container.uuid.is_some());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let original = fs::read(&path).unwrap();
    ConfigEntry::prepare_install(&path, None, true).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn install_appends_named_targets_without_changing_existing_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let original = json!({"runtime": {}, "container": {"name": "existing", "rootfs": "custom"}});
    fs::write(&path, json!([original]).to_string()).unwrap();
    let entry = ConfigEntry::prepare_install(&path, Some("new"), true).unwrap();
    let config = entry.load_for_install().unwrap();
    assert_eq!(config.container.name, "new");
    assert_eq!(
        config.container.rootfs_image,
        Some(dir.path().join("new.img"))
    );
    assert_eq!(read_document(&path).unwrap()[0], original);
    let persisted = fs::read(&path).unwrap();
    ConfigEntry::prepare_install(&path, Some("new"), false).unwrap();
    assert_eq!(fs::read(&path).unwrap(), persisted);
    assert!(ConfigEntry::prepare_install(&path, None, false).is_err());
    assert_eq!(fs::read(&path).unwrap(), persisted);
}

#[test]
fn install_accepts_empty_registry_but_preserves_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::write(&path, "[]").unwrap();
    let entry = ConfigEntry::prepare_install(&path, Some("first"), false).unwrap();
    assert_eq!(entry.load_for_install().unwrap().container.name, "first");
    for source in [
        "{",
        "{}",
        "null",
        "[null]",
        r#"[{"container":{"name":"one"}},{"container":{"name":"one"}}]"#,
    ] {
        fs::write(&path, source).unwrap();
        assert!(ConfigEntry::prepare_install(&path, Some("new"), false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
    fs::write(&path, "[]").unwrap();
    for name in ["", "..", "../escape", "/absolute"] {
        assert!(ConfigEntry::prepare_install(&path, Some(name), false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]");
    }
}

#[test]
fn install_updates_symlink_target_without_replacing_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("actual.json");
    let path = dir.path().join("config.json");
    fs::write(&target, "[]").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    ConfigEntry::prepare_install(&path, Some("new"), false).unwrap();
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        read_document(&target).unwrap()[0]["container"]["name"],
        "new"
    );
}
