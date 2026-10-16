use super::{create_mountpoint_file, make_bind_tree_read_only};
use kurumi_containerd_helper::fs::{MountFlags, mount, unmount};
use std::{fs, path::Path};

#[test]
fn existing_mountpoint_file_is_not_truncated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, "existing content").unwrap();

    create_mountpoint_file(&path).unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), "existing content");
}

#[test]
#[ignore = "requires root in a private mount namespace"]
fn recursive_read_only_preserves_flags_and_host_writes() {
    mount(
        None,
        Path::new("/"),
        None,
        MountFlags::REC | MountFlags::PRIVATE,
        None,
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    let restrictions = MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC;
    mount(
        Some(Path::new("tmpfs")),
        &source,
        Some("tmpfs"),
        restrictions,
        None,
    )
    .unwrap();
    fs::create_dir(source.join("child")).unwrap();
    mount(
        Some(Path::new("tmpfs")),
        &source.join("child"),
        Some("tmpfs"),
        restrictions,
        None,
    )
    .unwrap();
    mount(
        Some(&source),
        &target,
        None,
        MountFlags::BIND | MountFlags::REC,
        None,
    )
    .unwrap();
    make_bind_tree_read_only(&target).unwrap();
    for relative in ["", "child"] {
        let mounted = if relative.is_empty() {
            target.clone()
        } else {
            target.join(relative)
        };
        let error = fs::write(mounted.join("probe"), "blocked").unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EROFS));
        let mounts = procfs::process::Process::myself()
            .unwrap()
            .mountinfo()
            .unwrap();
        let entry = mounts
            .0
            .iter()
            .find(|entry| entry.mount_point == mounted)
            .unwrap();
        for flag in ["ro", "nosuid", "nodev", "noexec"] {
            assert!(entry.mount_options.contains_key(flag), "missing {flag}");
        }
        fs::write(source.join(relative).join("probe"), "host writable").unwrap();
    }
    unmount(&target, true).unwrap();
    unmount(&source, true).unwrap();
}
