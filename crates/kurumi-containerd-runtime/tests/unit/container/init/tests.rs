use std::os::unix::fs::PermissionsExt;

use super::*;

fn create(root: &Path, path: &str, content: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn detects_supported_init_families() {
    for (path, expected) in [
        ("usr/lib/systemd/systemd", InitSystem::Systemd),
        ("sbin/procd", InitSystem::Procd),
        ("sbin/openrc-init", InitSystem::Openrc),
        ("sbin/runit", InitSystem::Runit),
        ("bin/s6-svscan", InitSystem::S6),
    ] {
        let root = tempfile::tempdir().unwrap();
        create(root.path(), path, b"init");
        assert_eq!(detect(root.path(), Path::new("/sbin/init")), expected);
    }
}

#[test]
fn distinguishes_busybox_sysv_and_nix_wrappers() {
    let busybox = tempfile::tempdir().unwrap();
    create(busybox.path(), "sbin/init", b"#!/bin/sh\nexec busybox init");
    assert_eq!(
        detect(busybox.path(), Path::new("/sbin/init")),
        InitSystem::Busybox
    );

    let sysv = tempfile::tempdir().unwrap();
    create(sysv.path(), "sbin/init", b"binary init");
    assert_eq!(
        detect(sysv.path(), Path::new("/sbin/init")),
        InitSystem::Sysvinit
    );

    let nix = tempfile::tempdir().unwrap();
    create(
        nix.path(),
        "sbin/init",
        b"#!/bin/sh\nexec /nix/store/abc-finit",
    );
    assert_eq!(
        detect(nix.path(), Path::new("/sbin/init")),
        InitSystem::Unknown
    );
}

#[test]
fn validates_init_inside_rootfs() {
    let root = tempfile::tempdir().unwrap();
    create(root.path(), "sbin/init", b"init");
    let init = Init {
        path: PathBuf::from("/sbin/init"),
    };

    assert!(init.prepare(root.path()).is_err());
    fs::set_permissions(
        root.path().join("sbin/init"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(init.prepare(root.path()).is_ok());
}

#[test]
fn validates_init_symlinks_inside_rootfs() {
    let root = tempfile::tempdir().unwrap();
    create(root.path(), "usr/lib/systemd/systemd", b"init");
    fs::set_permissions(
        root.path().join("usr/lib/systemd/systemd"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::create_dir_all(root.path().join("sbin")).unwrap();
    std::os::unix::fs::symlink("/usr/lib/systemd/systemd", root.path().join("sbin/init")).unwrap();
    let init = Init {
        path: PathBuf::from("/sbin/init"),
    };

    assert!(init.prepare(root.path()).is_ok());
}

#[test]
fn rejects_init_symlink_outside_rootfs() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("sbin")).unwrap();
    std::os::unix::fs::symlink("../../../bin/sh", root.path().join("sbin/init")).unwrap();
    let init = Init {
        path: PathBuf::from("/sbin/init"),
    };

    assert!(init.prepare(root.path()).is_err());
}

#[test]
fn builds_sysvinit_poweroff_request() {
    let request = init_request();
    assert_eq!(
        i32::from_ne_bytes(request[0..4].try_into().unwrap()),
        0x0309_1969
    );
    assert_eq!(i32::from_ne_bytes(request[4..8].try_into().unwrap()), 1);
    assert_eq!(
        i32::from_ne_bytes(request[8..12].try_into().unwrap()),
        i32::from(b'0')
    );
    assert_eq!(request.len(), 384);
}
