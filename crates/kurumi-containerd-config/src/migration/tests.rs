use std::fs;

use serde_json::json;

use super::*;
use crate::ConfigEntry;

fn config() -> Config {
    serde_json::from_value(
        json!({"runtime": {"stop_timeout_seconds": 12}, "container": {
            "name": "source", "rootfs_image": "/source/device", "uuid": Uuid::new_v4(),
            "mounts": [{"source": "/old/data", "target": "/data", "read_only": true}],
            "environment_file": "/old/secrets", "environment": {"INLINE": "overridden"}
        }}),
    )
    .unwrap()
}

#[test]
fn import_creates_new_identity_and_rebinds_only_explicit_host_paths() {
    let home = tempfile::tempdir().unwrap();
    let registry = home.path().join("config.json");
    let existing = json!({"runtime": {}, "container": {"name": "existing", "rootfs": "old"}});
    fs::write(&registry, json!([existing]).to_string()).unwrap();
    let environment = home.path().join("local.env");
    fs::write(&environment, "LOCAL_ONLY=new\nINLINE=from-file").unwrap();
    let maps = [
        ("environment_file".into(), environment),
        ("mount.0".into(), home.path().into()),
    ];
    let original = config();
    let pending =
        PendingImport::prepare(&registry, original.clone(), Some("restored"), &maps, false)
            .unwrap();
    assert_eq!(
        pending.directory().metadata().unwrap().permissions().mode() & 0o777,
        0o700
    );
    let imported = &pending.config().container;
    assert!(imported.uuid.is_some());
    assert_ne!(imported.uuid, original.container.uuid);
    assert_eq!(imported.hostname, "restored");
    assert_eq!(
        imported.mounts[0].source,
        home.path().canonicalize().unwrap()
    );
    assert!(imported.mounts[0].read_only);
    assert_eq!(imported.mounts[0].target, Path::new("/data"));
    assert_eq!(imported.environment["LOCAL_ONLY"], "new");
    assert_eq!(imported.environment["INLINE"], "overridden");
    assert!(imported.rootfs_image.is_none());
    let target = imported.rootfs.as_ref().unwrap();
    fs::create_dir_all(target.join("sbin")).unwrap();
    fs::write(target.join("sbin/init"), "init").unwrap();
    pending.commit().unwrap();
    let document = read_install_document(&registry, false).unwrap();
    assert_eq!(document[0], existing);
    assert_eq!(
        document[1]["container"]["environment"],
        json!({"INLINE": "overridden"})
    );
    let entries = ConfigEntry::load(&registry).unwrap();
    let exported = ConfigEntry::select(&entries, Some("restored"))
        .unwrap()
        .load_for_export()
        .unwrap();
    assert!(!exported.container.environment.contains_key("LOCAL_ONLY"));
    assert_eq!(exported.container.hostname, "");
    assert!(PendingImport::prepare(&registry, original, Some("restored"), &maps, false).is_err());
    assert_eq!(read_install_document(&registry, false).unwrap(), document);
}

#[test]
fn import_errors_leave_no_target_or_registry_entry() {
    let home = tempfile::tempdir().unwrap();
    let registry = home.path().join("config.json");
    let mut source = config();
    source.container.environment_file = None;
    let mapping = ("mount.0".to_owned(), home.path().to_path_buf());
    for mappings in [
        vec![],
        vec![mapping.clone(), mapping.clone()],
        vec![mapping.clone(), ("extra".into(), home.path().into())],
        vec![("mount.0".into(), "relative".into())],
    ] {
        assert!(PendingImport::prepare(&registry, source.clone(), None, &mappings, false).is_err());
    }
    let mappings = [mapping];
    assert!(
        PendingImport::prepare(
            &registry,
            source.clone(),
            Some("../escape"),
            &mappings,
            true
        )
        .is_err()
    );
    let pending = PendingImport::prepare(&registry, source, None, &mappings, true).unwrap();
    let directory = pending.directory().to_owned();
    assert!(
        pending
            .config()
            .container
            .rootfs_image
            .as_ref()
            .unwrap()
            .starts_with(&directory)
    );
    assert!(pending.commit().is_err());
    assert!(!directory.exists());
    assert!(!registry.exists());
}

#[test]
fn overlapping_imports_preserve_other_entries_and_refuse_a_late_name_collision() {
    let home = tempfile::tempdir().unwrap();
    let registry = home.path().join("config.json");
    let prepare = |name| {
        let mut source = config();
        source.container.mounts.clear();
        source.container.environment_file = None;
        let pending = PendingImport::prepare(&registry, source, Some(name), &[], false).unwrap();
        fs::create_dir(pending.config().container.rootfs.as_ref().unwrap()).unwrap();
        pending
    };
    let first = prepare("first");
    let duplicate = prepare("first");
    prepare("second").commit().unwrap();
    first.commit().unwrap();
    let discarded = duplicate.directory().to_owned();
    assert!(duplicate.commit().is_err());
    assert!(!discarded.exists());
    let names: Vec<_> = ConfigEntry::load(&registry)
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["second", "first"]);
}
