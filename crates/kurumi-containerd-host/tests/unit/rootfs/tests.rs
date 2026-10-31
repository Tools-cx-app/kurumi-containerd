// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;
use std::os::unix::{
    fs::{MetadataExt, PermissionsExt, symlink},
    net::UnixListener,
};

use tempfile::{TempDir, tempdir};

fn backup_fixture() -> (TempDir, Rootfs) {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source");
    fs::create_dir_all(source.join("bin")).unwrap();
    fs::write(source.join("bin/init"), b"executable rootfs content").unwrap();
    fs::set_permissions(source.join("bin/init"), fs::Permissions::from_mode(0o755)).unwrap();
    symlink("init", source.join("bin/sh")).unwrap();
    fs::hard_link(source.join("bin/init"), source.join("bin/linked-init")).unwrap();
    let rootfs = Rootfs::from_paths(Some(&source), None, &directory.path().join("mount"));
    (directory, rootfs)
}

#[test]
fn constructs_rootfs_from_paths_without_config() {
    let rootfs = Rootfs::from_paths(Some(Path::new("/rootfs")), None, Path::new("/mount/test"));

    assert_eq!(rootfs.configured.as_deref(), Some(Path::new("/rootfs")));
    assert_eq!(rootfs.image, None);
    assert_eq!(rootfs.mountpoint, Path::new("/mount/test"));
}

#[test]
fn backup_formats_restore_contents_permissions_and_symlinks() {
    let (directory, rootfs) = backup_fixture();
    for extension in ["tar", "xz", "zst", "zip", "tar.xz", "tar.zst"] {
        let output = directory.path().join(format!("rootfs.{extension}"));
        rootfs.backup(&output).unwrap();
        assert_eq!(fs::metadata(&output).unwrap().mode() & 0o777, 0o600);
        let restored = directory.path().join(format!("restored-{extension}"));
        fs::create_dir(&restored).unwrap();
        crate::archive::extract(&output, &restored).unwrap();
        assert_eq!(
            fs::read(restored.join("bin/init")).unwrap(),
            b"executable rootfs content"
        );
        assert_eq!(
            fs::read(restored.join("bin/sh")).unwrap(),
            b"executable rootfs content"
        );
        assert_eq!(
            fs::read_link(restored.join("bin/sh")).unwrap(),
            Path::new("init")
        );
        assert_eq!(
            fs::metadata(restored.join("bin/init")).unwrap().mode() & 0o777,
            0o755
        );
        let original = fs::metadata(rootfs.configured.as_ref().unwrap().join("bin/init")).unwrap();
        let restored_file = fs::metadata(restored.join("bin/init")).unwrap();
        if extension != "zip" {
            assert_eq!(
                restored_file.ino(),
                fs::metadata(restored.join("bin/linked-init"))
                    .unwrap()
                    .ino()
            );
            assert_eq!(restored_file.uid(), original.uid());
            assert_eq!(restored_file.gid(), original.gid());
        }
    }
}

#[test]
fn backup_rejects_existing_destinations_and_rootfs_paths() {
    let (directory, rootfs) = backup_fixture();
    let source = rootfs.configured.as_ref().unwrap();
    let existing = directory.path().join("existing.tar");
    fs::write(&existing, b"keep existing backup").unwrap();
    assert!(rootfs.backup(&existing).is_err());
    assert_eq!(fs::read(&existing).unwrap(), b"keep existing backup");
    let output_link = directory.path().join("output-link.tar");
    symlink(&existing, &output_link).unwrap();
    assert!(rootfs.backup(&output_link).is_err());
    assert_eq!(fs::read(&existing).unwrap(), b"keep existing backup");
    let dangling = directory.path().join("dangling.tar");
    symlink("absent", &dangling).unwrap();
    assert!(rootfs.backup(&dangling).is_err());
    assert!(
        dangling
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(rootfs.backup(&source.join("inside.tar")).is_err());
    assert!(!source.join("inside.tar").exists());
    let alias = directory.path().join("source-alias");
    symlink(source, &alias).unwrap();
    assert!(rootfs.backup(&alias.join("inside.tar")).is_err());
    assert!(
        rootfs
            .backup(&directory.path().join("unsupported.gz"))
            .is_err()
    );
}

#[test]
fn failed_backup_removes_partial_archives() {
    let (directory, rootfs) = backup_fixture();
    let source = rootfs.configured.as_ref().unwrap();
    let _socket = UnixListener::bind(source.join("socket")).unwrap();
    for extension in ["tar", "xz", "zst", "zip"] {
        let output = directory.path().join(format!("failed.{extension}"));
        assert!(rootfs.backup(&output).is_err());
        assert!(!output.exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn zip_backup_rejects_unsafe_and_unrepresentable_links_and_paths() {
    let (directory, rootfs) = backup_fixture();
    let source = rootfs.configured.as_ref().unwrap();
    let output = directory.path().join("failed.zip");
    for target in ["/bin/init", "../../outside"] {
        symlink(target, source.join("unsafe")).unwrap();
        assert!(rootfs.backup(&output).is_err());
        assert!(!output.exists());
        fs::remove_file(source.join("unsafe")).unwrap();
    }
    for name in ["a\\b", "C:drive"] {
        fs::write(source.join(name), b"must not normalize this name").unwrap();
        assert!(rootfs.backup(&output).is_err());
        assert!(!output.exists());
        fs::remove_file(source.join(name)).unwrap();
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn tar_backup_preserves_file_xattrs_and_records_directory_xattrs() {
    let (directory, rootfs) = backup_fixture();
    let source = rootfs.configured.as_ref().unwrap();
    match xattr::set(
        source.join("bin/init"),
        "user.backup-test",
        b"value\0binary",
    ) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(libc::ENOTSUP) => return,
        Err(error) => panic!("failed to set test extended attribute: {error}"),
    }
    xattr::set(source.join("bin"), "user.backup-directory", b"directory").unwrap();
    let output = directory.path().join("attrs.tar");
    rootfs.backup(&output).unwrap();
    let restored = directory.path().join("restored");
    fs::create_dir(&restored).unwrap();
    crate::archive::extract(&output, &restored).unwrap();
    assert_eq!(
        xattr::get(restored.join("bin/init"), "user.backup-test").unwrap(),
        Some(b"value\0binary".to_vec())
    );
    let mut archive = tar::Archive::new(File::open(output).unwrap());
    let mut found = false;
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap() == Path::new("bin") {
            found = entry.pax_extensions().unwrap().unwrap().any(|attribute| {
                let attribute = attribute.unwrap();
                attribute.key_bytes() == b"SCHILY.xattr.user.backup-directory"
                    && attribute.value_bytes() == b"directory"
            });
        }
    }
    assert!(found, "directory metadata must be retained in the backup");
}

#[test]
fn backup_rejects_live_kernel_filesystems_without_writing_output() {
    let directory = tempdir().unwrap();
    let rootfs = Rootfs::from_paths(
        Some(Path::new("/proc")),
        None,
        &directory.path().join("mount"),
    );
    assert!(rootfs.backup(&directory.path().join("proc.tar")).is_err());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}
