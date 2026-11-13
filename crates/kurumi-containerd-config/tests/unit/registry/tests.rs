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
