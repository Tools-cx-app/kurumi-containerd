use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;

use serde_json::json;

use super::*;
use crate::rootfs::Rootfs;

fn package(directory: &Path) -> PathBuf {
    let root = directory.join("source");
    fs::create_dir_all(root.join("sbin")).unwrap();
    fs::write(root.join("sbin/init"), b"init body").unwrap();
    fs::set_permissions(root.join("sbin/init"), fs::Permissions::from_mode(0o755)).unwrap();
    symlink("sbin/init", root.join("link")).unwrap();
    let data = directory.join("external-data");
    fs::create_dir(&data).unwrap();
    fs::write(data.join("private"), "external contents").unwrap();
    let config = serde_json::from_value(json!({"runtime": {}, "container": {
        "name": "original", "rootfs": root, "uuid": uuid::Uuid::new_v4(), "network": "none",
        "mounts": [{"source": data, "target": "/data", "read_only": true}]
    }}))
    .unwrap();
    let path = directory.join("system.kurumi");
    ContainerPackage::export(&root, &path, config).unwrap();
    path
}

#[test]
fn exports_private_portable_package_and_installs_verified_rootfs() {
    let directory = tempfile::tempdir().unwrap();
    let path = package(directory.path());
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    let mut package = ContainerPackage::open(&path).unwrap();
    let config = package.manifest.config.clone();
    assert_eq!(config.container.name, "original");
    assert!(config.container.uuid.is_none() && config.container.rootfs.is_none());
    assert_eq!(
        config.external_paths()[0].mount.unwrap().target,
        Path::new("/data")
    );
    let archive = package.verified_rootfs(directory.path()).unwrap();
    let restored = directory.path().join("restored");
    Rootfs::from_paths(Some(&restored), None, directory.path())
        .install(archive.path(), None, false, |_| Ok(()))
        .unwrap();
    assert_eq!(fs::read(restored.join("sbin/init")).unwrap(), b"init body");
    assert_eq!(
        fs::read_link(restored.join("link")).unwrap(),
        Path::new("sbin/init")
    );
    assert!(!restored.join("data/private").exists());
    let root = directory.path().join("source");
    assert!(ContainerPackage::export(&root, &path, config.clone()).is_err());
    assert!(ContainerPackage::export(&root, &root.join("self.kurumi"), config).is_err());
}

#[test]
fn rejects_corrupt_payload_and_manifest_before_install() {
    let directory = tempfile::tempdir().unwrap();
    let path = package(directory.path());
    let mut bytes = fs::read(&path).unwrap();
    let original = bytes.clone();
    bytes[525] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let mut package = ContainerPackage::open(&path).unwrap();
    assert!(
        package
            .verified_rootfs(directory.path())
            .unwrap_err()
            .to_string()
            .contains("SHA-256")
    );
    let mut bytes = original;
    let name = bytes
        .windows(8)
        .position(|window| window == b"original")
        .unwrap();
    bytes[name] = b'x';
    fs::write(&path, &bytes).unwrap();
    assert!(
        ContainerPackage::open(&path)
            .err()
            .unwrap()
            .to_string()
            .contains("manifest SHA-256")
    );
    fs::write(&path, &bytes[..600]).unwrap();
    assert!(ContainerPackage::open(&path).is_err());
}
