use std::io::{Cursor, Write};

use tempfile::tempdir;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;

#[test]
fn extracts_zip_file_with_mode() {
    let directory = tempdir().unwrap();
    let archive_path = directory.path().join("rootfs.zip");
    let mut archive = ZipWriter::new(File::create(&archive_path).unwrap());
    archive
        .start_file(
            "sbin/init",
            SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
    archive.write_all(b"init").unwrap();
    archive.finish().unwrap();
    let target = directory.path().join("rootfs");
    fs::create_dir(&target).unwrap();

    extract(&archive_path, &target).unwrap();

    assert_eq!(fs::read(target.join("sbin/init")).unwrap(), b"init");
    assert_eq!(
        fs::metadata(target.join("sbin/init"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[test]
fn rejects_zip_parent_traversal() {
    assert!(ensure_safe_path(Path::new("/rootfs"), Path::new("../escape")).is_err());
}

#[test]
fn rejects_zip_symlink_escape() {
    assert!(ensure_safe_link(Path::new("link"), Path::new("../escape")).is_err());
    assert!(ensure_safe_link(Path::new("usr/link"), Path::new("../lib")).is_ok());
    assert!(ensure_safe_link(Path::new("usr/link"), Path::new("../../escape")).is_err());
}

#[test]
fn extracts_tar_and_compressed_tar_aliases() {
    let directory = tempdir().unwrap();
    let tar_data = tar_data();
    for (name, kind) in [
        ("rootfs-tar.bin", ArchiveKind::Tar),
        ("rootfs-gzip.bin", ArchiveKind::Gzip),
        ("rootfs-xz.bin", ArchiveKind::Xz),
        ("rootfs-zstd.bin", ArchiveKind::Zstd),
    ] {
        let archive_path = directory.path().join(name);
        let encoded = match kind {
            ArchiveKind::Tar => tar_data.clone(),
            ArchiveKind::Gzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(&tar_data).unwrap();
                encoder.finish().unwrap()
            }
            ArchiveKind::Xz => {
                let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 1);
                encoder.write_all(&tar_data).unwrap();
                encoder.finish().unwrap()
            }
            ArchiveKind::Zstd => zstd::stream::encode_all(Cursor::new(&tar_data), 1).unwrap(),
            ArchiveKind::Zip => unreachable!(),
        };
        fs::write(&archive_path, encoded).unwrap();
        let target = directory.path().join(format!("target-{name}"));
        fs::create_dir(&target).unwrap();

        extract(&archive_path, &target).unwrap();
        assert_eq!(fs::read(target.join("sbin/init")).unwrap(), b"init");
    }
}

#[test]
fn rejects_unknown_archive_format() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("rootfs.tar.zst");
    fs::write(&archive, b"not an archive").unwrap();
    let target = directory.path().join("rootfs");
    fs::create_dir(&target).unwrap();

    assert!(extract(&archive, &target).is_err());
}

#[test]
fn rejects_tar_special_files() {
    let directory = tempdir().unwrap();
    let archive_path = directory.path().join("special.tar");
    let mut archive = tar::Builder::new(File::create(&archive_path).unwrap());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::fifo());
    header.set_size(0);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();
    archive
        .append_data(&mut header, "run/fifo", &[][..])
        .unwrap();
    archive.finish().unwrap();
    let target = directory.path().join("rootfs");
    fs::create_dir(&target).unwrap();

    assert!(extract(&archive_path, &target).is_err());
    assert!(!target.join("run/fifo").exists());
}

#[test]
fn zip_file_cannot_overwrite_symlink() {
    let directory = tempdir().unwrap();
    let archive_path = directory.path().join("rootfs.zip");
    let mut archive = ZipWriter::new(File::create(&archive_path).unwrap());
    archive
        .start_file(
            "bin/tool",
            SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
    archive.write_all(b"replacement").unwrap();
    archive.finish().unwrap();
    let target = directory.path().join("rootfs");
    fs::create_dir_all(target.join("bin")).unwrap();
    fs::write(target.join("real"), "original").unwrap();
    symlink("../real", target.join("bin/tool")).unwrap();

    assert!(extract(&archive_path, &target).is_err());
    assert_eq!(fs::read_to_string(target.join("real")).unwrap(), "original");
}

#[test]
fn zip_directory_cannot_overwrite_symlink() {
    let directory = tempdir().unwrap();
    let archive_path = directory.path().join("rootfs.zip");
    let mut archive = ZipWriter::new(File::create(&archive_path).unwrap());
    archive
        .add_directory(
            "real/",
            SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
    archive
        .add_symlink("usr", "real", SimpleFileOptions::default())
        .unwrap();
    archive
        .add_directory("usr/", SimpleFileOptions::default().unix_permissions(0o700))
        .unwrap();
    archive.finish().unwrap();
    let target = directory.path().join("rootfs");
    fs::create_dir(&target).unwrap();

    assert!(extract(&archive_path, &target).is_err());
    assert_eq!(
        fs::metadata(target.join("real"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[test]
fn recognizes_tar_header_without_extension() {
    let directory = tempdir().unwrap();
    let archive = directory.path().join("rootfs.data");
    fs::write(&archive, tar_data()).unwrap();
    let target = directory.path().join("rootfs");
    fs::create_dir(&target).unwrap();

    extract(&archive, &target).unwrap();

    assert!(target.join("sbin/init").is_file());
}

#[test]
fn tar_cannot_write_through_symlink_outside_rootfs() {
    let directory = tempdir().unwrap();
    let archive_path = directory.path().join("escape.tar");
    let mut archive = tar::Builder::new(File::create(&archive_path).unwrap());
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    link.set_mode(0o777);
    link.set_uid(0);
    link.set_gid(0);
    link.set_link_name("../../outside").unwrap();
    link.set_cksum();
    archive.append_data(&mut link, "var/link", &[][..]).unwrap();
    let mut file = tar::Header::new_gnu();
    file.set_size(6);
    file.set_mode(0o644);
    file.set_uid(0);
    file.set_gid(0);
    file.set_cksum();
    archive
        .append_data(&mut file, "var/link/escape", &b"escape"[..])
        .unwrap();
    archive.finish().unwrap();
    let target = directory.path().join("rootfs");
    fs::create_dir(&target).unwrap();

    assert!(extract(&archive_path, &target).is_err());
    assert!(!directory.path().join("outside/escape").exists());
}

fn tar_data() -> Vec<u8> {
    let mut data = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut data);
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
    }
    data
}
