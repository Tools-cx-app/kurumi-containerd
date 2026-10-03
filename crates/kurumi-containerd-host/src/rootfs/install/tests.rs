use tempfile::tempdir;

use super::super::detect_filesystem;
use super::*;

#[test]
fn directory_install_commits_valid_archive() {
    let directory = tempdir().unwrap();
    let archive = make_archive(directory.path());
    let target = directory.path().join("rootfs");
    install_directory(&archive, &target, false, validate_init).unwrap();
    assert_eq!(fs::read(target.join("sbin/init")).unwrap(), b"init");
}

#[test]
fn existing_directory_requires_force() {
    let directory = tempdir().unwrap();
    let archive = make_archive(directory.path());
    let target = directory.path().join("rootfs");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("old"), "old").unwrap();
    assert!(install_directory(&archive, &target, false, validate_init).is_err());
    assert!(target.join("old").exists());
    install_directory(&archive, &target, true, validate_init).unwrap();
    assert!(!target.join("old").exists());
    assert!(target.join("sbin/init").exists());
}

#[test]
fn failed_validation_preserves_existing_directory() {
    let directory = tempdir().unwrap();
    let archive = make_archive(directory.path());
    let target = directory.path().join("rootfs");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("old"), "old").unwrap();
    assert!(install_directory(&archive, &target, true, |_| bail!("invalid")).is_err());
    assert_eq!(fs::read_to_string(target.join("old")).unwrap(), "old");
}

#[test]
fn rejects_symlink_target_even_with_force() {
    let directory = tempdir().unwrap();
    let archive = make_archive(directory.path());
    let outside = directory.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let target = directory.path().join("rootfs");
    std::os::unix::fs::symlink(&outside, &target).unwrap();
    assert!(install_directory(&archive, &target, true, validate_init).is_err());
}

#[test]
fn directory_install_rejects_existing_file() {
    let directory = tempdir().unwrap();
    let archive = make_archive(directory.path());
    let target = directory.path().join("rootfs");
    fs::write(&target, "not a directory").unwrap();
    assert!(install_directory(&archive, &target, true, validate_init).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "not a directory");
}

#[test]
fn committed_directory_is_traversable() {
    let directory = tempdir().unwrap();
    let archive = make_archive(directory.path());
    let target = directory.path().join("rootfs");
    install_directory(&archive, &target, false, validate_init).unwrap();
    assert_eq!(
        fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn rejects_world_writable_target_parent() {
    let directory = tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o777)).unwrap();
    let archive = make_archive(directory.path());
    let target = directory.path().join("rootfs");
    assert!(install_directory(&archive, &target, false, validate_init).is_err());
}

#[test]
fn sparse_file_has_requested_logical_size() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("image");
    let file = File::create(&path).unwrap();
    file.set_len(64 * 1024 * 1024).unwrap();
    let metadata = file.metadata().unwrap();
    assert_eq!(metadata.len(), 64 * 1024 * 1024);
    assert!(std::os::unix::fs::MetadataExt::blocks(&metadata) * 512 < metadata.len());
}

#[test]
fn formatted_sparse_file_is_ext4() {
    if ![
        "/usr/sbin/mke2fs",
        "/sbin/mke2fs",
        "/system/bin/mke2fs",
        "/usr/sbin/mkfs.ext4",
        "/sbin/mkfs.ext4",
        "/system/bin/mkfs.ext4",
    ]
    .iter()
    .any(|path| Path::new(path).is_file())
    {
        return;
    }
    let directory = tempdir().unwrap();
    let path = directory.path().join("rootfs.img");
    let file = File::create(&path).unwrap();
    file.set_len(64 * 1024 * 1024).unwrap();
    format_ext4(&path).unwrap();
    assert_eq!(detect_filesystem(&path).unwrap(), "ext4");
    let metadata = fs::metadata(path).unwrap();
    assert_eq!(metadata.len(), 64 * 1024 * 1024);
    assert!(std::os::unix::fs::MetadataExt::blocks(&metadata) * 512 < metadata.len());
}

#[test]
fn size_matches_configured_target_type() {
    let directory = tempdir().unwrap();
    let archive = make_archive(directory.path());
    let rootfs = Rootfs {
        configured: Some(directory.path().join("rootfs")),
        image: None,
        mountpoint: directory.path().join("mount"),
    };
    assert!(
        rootfs
            .install(&archive, Some(1024), false, validate_init)
            .is_err()
    );
    let rootfs = Rootfs {
        configured: None,
        image: Some(directory.path().join("rootfs.img")),
        mountpoint: directory.path().join("mount"),
    };
    assert!(
        rootfs
            .install(&archive, None, false, validate_init)
            .is_err()
    );
}

fn validate_init(path: &Path) -> Result<()> {
    ensure!(path.join("sbin/init").is_file(), "missing init");
    Ok(())
}

fn make_archive(directory: &Path) -> PathBuf {
    let path = directory.join(format!("rootfs-{}.tar", Uuid::new_v4()));
    let file = File::create(&path).unwrap();
    let mut archive = tar::Builder::new(file);
    let mut header = tar::Header::new_gnu();
    header.set_size(4);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();
    archive
        .append_data(&mut header, "sbin/init", &b"init"[..])
        .unwrap();
    archive.finish().unwrap();
    path
}
