// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::os::unix::fs::symlink;

use super::{
    state::{NatLease, NatLeaseOwner},
    *,
};

#[test]
fn network_cleanup_does_not_create_state_files() {
    let directory = tempfile::tempdir().unwrap();
    drop(Network::empty(NetworkState::new().unwrap()));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn no_network_mode_does_not_require_iproute2() {
    Network::setup_child_with(
        "test",
        NetworkMode::None,
        &kurumi_containerd_config::NetworkConfig::default(),
        "none",
    )
    .unwrap();
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

#[test]
fn gateway_configures_static_address_and_default_route() {
    let options = kurumi_containerd_config::NetworkConfig {
        address: "192.0.2.2".parse().unwrap(),
        gateway: "192.0.2.1".parse().unwrap(),
        prefix: 24,
        ..Default::default()
    };
    for mode in [NetworkMode::Gateway, NetworkMode::Nat, NetworkMode::Dhcp] {
        let mut commands = Vec::new();
        setup_child_interfaces(mode, &options, "peer", |args| {
            commands.push(args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            &commands[..3],
            &[
                vec!["link", "set", "lo", "up"],
                vec!["link", "set", "peer", "name", "eth0"],
                vec!["link", "set", "eth0", "up"],
            ]
        );
        if mode == NetworkMode::Dhcp {
            assert_eq!(commands.len(), 3);
        } else {
            assert_eq!(
                &commands[3..],
                &[
                    vec!["address", "add", "192.0.2.2/24", "dev", "eth0"],
                    vec!["route", "add", "default", "via", "192.0.2.1"],
                ]
            );
        }
    }
}
