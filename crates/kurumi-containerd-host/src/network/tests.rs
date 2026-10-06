use std::os::unix::fs::symlink;

use super::*;

#[test]
fn network_state_uses_supplied_directory_for_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    if kurumi_containerd_helper::process::effective_uid() == 0 {
        drop(Network::empty(directory.path()));
        assert!(directory.path().join("network.lock").is_file());
    } else {
        let error = network_state_dir(directory.path()).unwrap_err();
        assert!(error.to_string().contains("must be owned by root"));
    }
}

#[test]
fn network_state_rejects_untrusted_directories() {
    let directory = tempfile::tempdir().unwrap();
    let link = directory.path().join("link");
    symlink(directory.path(), &link).unwrap();
    assert!(network_state_dir(&link).is_err());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(network_state_dir(directory.path()).is_err());
}

#[test]
fn replaces_dangling_resolv_conf_symlink() {
    let rootfs = tempfile::tempdir().unwrap();
    fs::create_dir(rootfs.path().join("etc")).unwrap();
    symlink(
        "../run/systemd/resolve/stub-resolv.conf",
        rootfs.path().join("etc/resolv.conf"),
    )
    .unwrap();
    let config = test_config();

    Network::write_dns(&config, rootfs.path()).unwrap();

    assert_eq!(
        fs::read_to_string(rootfs.path().join("etc/resolv.conf")).unwrap(),
        "nameserver 1.1.1.1\n"
    );
}

#[test]
fn replaces_live_resolv_conf_symlink() {
    let rootfs = tempfile::tempdir().unwrap();
    fs::create_dir(rootfs.path().join("etc")).unwrap();
    fs::write(rootfs.path().join("etc/target"), "keep me\n").unwrap();
    symlink("target", rootfs.path().join("etc/resolv.conf")).unwrap();
    let config = test_config();

    Network::write_dns(&config, rootfs.path()).unwrap();

    assert_eq!(
        fs::read_to_string(rootfs.path().join("etc/resolv.conf")).unwrap(),
        "nameserver 1.1.1.1\n"
    );
    assert_eq!(
        fs::read_to_string(rootfs.path().join("etc/target")).unwrap(),
        "keep me\n"
    );
    assert!(
        !fs::symlink_metadata(rootfs.path().join("etc/resolv.conf"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

fn test_config() -> Config {
    toml::from_str(
        "[runtime]\n\n[container]\nname = 'test'\nrootfs = '/tmp'\n\n[container.network_options]\ndns = ['1.1.1.1']\n",
    )
    .unwrap()
}

#[test]
fn rejects_stale_nat_lease_owner_identity() {
    let owner = NatLeaseOwner::current().unwrap();
    assert!(owner.is_live(&owner.host_boot_id));

    let stale_owner = NatLeaseOwner {
        start_time: owner.start_time.wrapping_add(1),
        ..owner
    };
    assert!(!stale_owner.is_live(&stale_owner.host_boot_id));
}

#[test]
fn preserves_restore_state_from_legacy_nat_lease() {
    let lease: NatLease = serde_json::from_str(r#"{"users":2,"restore_disabled":true}"#).unwrap();
    assert_eq!(lease.owners.len(), 0);
    assert!(lease.restore_disabled);
}
