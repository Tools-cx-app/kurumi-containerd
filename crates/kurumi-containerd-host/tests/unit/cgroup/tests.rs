// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{fs::File, path::PathBuf};

use kurumi_containerd_config::ResourceConfig;
use procfs::{FromRead, ProcessCGroups, process::MountInfo};

use super::{
    Cgroup, CgroupMembership, Controller, cgroup_required, enable_controllers,
    execution_cgroup_paths, parse_cgroup1_roots, requested_controllers, write_limit,
};

#[test]
fn cleanup_removes_nested_groups_and_retains_failed_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("container");
    std::fs::create_dir_all(root.join("system.slice/service")).unwrap();
    let mut cgroup = Cgroup {
        paths: vec![root.clone()],
        unified: true,
    };
    cgroup.remove().unwrap();
    assert!(!root.exists());
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("file"), "busy").unwrap();
    cgroup.paths.push(root.clone());
    assert!(cgroup.remove().is_err());
    std::fs::remove_file(root.join("file")).unwrap();
    cgroup.remove().unwrap();
    assert!(!root.exists());
}

#[test]
fn enables_only_missing_requested_controllers() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("cgroup.controllers"),
        "cpu memory pids",
    )
    .unwrap();
    let control = directory.path().join("cgroup.subtree_control");
    std::fs::write(&control, "cpu").unwrap();
    enable_controllers(directory.path(), &[Controller::Cpu, Controller::Memory]).unwrap();
    assert_eq!(std::fs::read_to_string(&control).unwrap(), "+memory");

    std::fs::write(&control, "cpu memory").unwrap();
    enable_controllers(directory.path(), &[Controller::Memory]).unwrap();
    assert_eq!(std::fs::read_to_string(&control).unwrap(), "cpu memory");

    std::fs::write(directory.path().join("cgroup.controllers"), "cpu").unwrap();
    assert!(enable_controllers(directory.path(), &[Controller::Memory]).is_err());
    assert_eq!(std::fs::read_to_string(control).unwrap(), "cpu memory");
}

#[test]
fn systemd_requires_cgroup_without_resource_limits() {
    let resources = ResourceConfig::default();

    assert!(cgroup_required(&resources, true));
    assert_eq!(
        requested_controllers(&resources, true),
        vec![Controller::Pids]
    );
}

#[test]
fn custom_init_skips_cgroup_without_resource_limits() {
    assert!(!cgroup_required(&ResourceConfig::default(), false));
}

#[test]
fn parses_controller_mounts_from_mountinfo() {
    let mountinfo = "29 23 0:26 / /sys/fs/cgroup/memory rw - cgroup cgroup rw,memory\n30 23 0:27 / /sys/fs/cgroup/cpu rw - cgroup cgroup rw,cpu,cpuacct\n31 23 0:28 / /sys/fs/cgroup/pids rw - cgroup cgroup rw,pids\n";
    let mountinfo = mountinfo
        .lines()
        .map(|line| MountInfo::from_line(line).unwrap())
        .collect::<Vec<_>>();
    let roots = parse_cgroup1_roots(&mountinfo);
    assert_eq!(
        roots.get(&Controller::Memory),
        Some(&PathBuf::from("/sys/fs/cgroup/memory"))
    );
    assert_eq!(
        roots.get(&Controller::Cpu),
        Some(&PathBuf::from("/sys/fs/cgroup/cpu"))
    );
    assert_eq!(
        roots.get(&Controller::Pids),
        Some(&PathBuf::from("/sys/fs/cgroup/pids"))
    );
}

#[test]
fn resets_removed_limit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("memory.max");
    std::fs::write(&path, "1048576").unwrap();

    write_limit(&path, None, "max").unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), "max");
}

#[test]
fn cleanup_continues_after_error() {
    let directory = tempfile::tempdir().unwrap();
    let blocked = directory.path().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("file"), "content").unwrap();
    let removable = directory.path().join("removable");
    std::fs::create_dir(&removable).unwrap();
    let mut cgroup = Cgroup {
        paths: vec![blocked, removable.clone()],
        unified: false,
    };

    assert!(cgroup.remove().is_err());
    assert!(!removable.exists());
}

fn execution_paths(groups: &str, mounts: &str) -> crate::Result<Vec<PathBuf>> {
    let groups = ProcessCGroups::from_read(groups.as_bytes()).unwrap();
    let mounts = mounts
        .lines()
        .map(|line| MountInfo::from_line(line).unwrap())
        .collect::<Vec<_>>();
    execution_cgroup_paths("demo", &groups.0, &mounts)
}

#[test]
fn execution_uses_actual_v2_membership_including_systemd_descendants() {
    let paths = execution_paths(
        "0::/kurumi-containerd/demo/init.scope\n",
        "29 23 0:26 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n",
    )
    .unwrap();
    assert_eq!(
        paths,
        vec![PathBuf::from("/sys/fs/cgroup/kurumi-containerd/demo")]
    );
    assert_eq!(
        execution_paths(
            "0::/user.slice\n",
            "29 23 0:26 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n",
        )
        .unwrap(),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn execution_retains_every_active_v1_controller_without_resource_configuration() {
    let paths = execution_paths(
        "4:cpu,cpuacct:/kurumi-containerd/demo\n3:memory:/kurumi-containerd/demo/services\n2:pids:/user.slice\n0::/user.slice\n",
        "29 23 0:26 / /sys/fs/cgroup/cpu rw - cgroup cgroup rw,cpu,cpuacct\n30 23 0:27 / /sys/fs/cgroup/memory rw - cgroup cgroup rw,memory\n31 23 0:28 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n",
    ).unwrap();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/sys/fs/cgroup/cpu/kurumi-containerd/demo"),
            PathBuf::from("/sys/fs/cgroup/memory/kurumi-containerd/demo"),
        ]
    );
}

#[test]
fn execution_resolves_delegated_mount_roots_and_rejects_missing_hierarchies() {
    let paths = execution_paths(
        "0::/kurumi-containerd/demo/init.scope\n",
        "29 23 0:26 /kurumi-containerd /sys/fs/cgroup/delegated rw - cgroup2 cgroup rw\n",
    )
    .unwrap();
    assert_eq!(paths, vec![PathBuf::from("/sys/fs/cgroup/delegated/demo")]);
    assert!(execution_paths("0::/kurumi-containerd/demo\n", "").is_err());
    assert!(execution_paths("0::/kurumi-containerd/demo/../../outside\n", "").is_err());
    assert_eq!(
        execution_paths("0::/kurumi-containerd/demo-other\n", "").unwrap(),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn captured_execution_membership_keeps_limits_and_survives_directory_rename() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original");
    std::fs::create_dir(&original).unwrap();
    std::fs::write(original.join("memory.max"), "4194304").unwrap();
    let membership = CgroupMembership {
        directories: vec![File::open(&original).unwrap()],
    };
    let renamed = directory.path().join("renamed");
    std::fs::rename(original, &renamed).unwrap();
    membership.attach(42).unwrap();
    assert_eq!(
        std::fs::read_to_string(renamed.join("kurumi-exec/cgroup.procs")).unwrap(),
        "42"
    );
    assert_eq!(
        std::fs::read_to_string(renamed.join("memory.max")).unwrap(),
        "4194304"
    );
    drop(membership);
    assert!(renamed.is_dir());
}
