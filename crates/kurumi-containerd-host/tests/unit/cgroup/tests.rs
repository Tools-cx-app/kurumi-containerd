// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{fs::File, os::unix::fs::MetadataExt, path::PathBuf};

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
        shared: None,
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
        shared: None,
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

#[test]
fn shared_mode_requests_no_controllers_without_limits() {
    assert_eq!(requested_controllers(&ResourceConfig::default(), false), []);
    let resources = ResourceConfig {
        pids: Some(32),
        ..ResourceConfig::default()
    };
    assert_eq!(
        requested_controllers(&resources, false),
        vec![Controller::Pids]
    );
}

#[test]
fn shared_membership_resolves_monitor_mount_root_and_rejects_traversal() {
    let mounts =
        [
            MountInfo::from_line("29 23 0:26 /delegated /sys/fs/cgroup rw - cgroup2 cgroup rw")
                .unwrap(),
        ];
    assert_eq!(
        super::shared_cgroup_path("/delegated/kurumi-containerd/demo", &mounts).unwrap(),
        PathBuf::from("/sys/fs/cgroup/kurumi-containerd/demo")
    );
    for membership in [
        "relative",
        "/",
        "/delegated/../outside",
        "/other/kurumi-containerd/demo",
    ] {
        assert!(
            super::shared_cgroup_path(membership, &mounts).is_err(),
            "accepted {membership}"
        );
    }
}

#[test]
fn recursive_membership_deduplicates_and_survives_rename() {
    let temporary = tempfile::tempdir().unwrap();
    let original = temporary.path().join("original");
    std::fs::create_dir_all(original.join("services/nested")).unwrap();
    std::fs::write(original.join("cgroup.procs"), "41\n42\n").unwrap();
    std::fs::write(original.join("services/cgroup.procs"), "42\n43\n").unwrap();
    std::fs::write(original.join("services/nested/cgroup.procs"), "44\n").unwrap();
    let captured = File::open(&original).unwrap();
    std::fs::rename(&original, temporary.path().join("renamed")).unwrap();
    assert_eq!(
        super::cgroup_processes(&captured).unwrap(),
        vec![41, 42, 43, 44]
    );
}

#[test]
fn recursive_membership_rejects_bad_files_and_does_not_follow_symlinks() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = File::open(temporary.path()).unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "12\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("cgroup.procs"), "99\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), temporary.path().join("escape")).unwrap();
    assert_eq!(super::cgroup_processes(&directory).unwrap(), vec![12]);
    for contents in ["0\n", "-1\n", "not a pid\n"] {
        std::fs::write(temporary.path().join("cgroup.procs"), contents).unwrap();
        assert!(super::cgroup_processes(&directory).is_err());
    }
    std::fs::remove_file(temporary.path().join("cgroup.procs")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("cgroup.procs"),
        temporary.path().join("cgroup.procs"),
    )
    .unwrap();
    assert!(super::cgroup_processes(&directory).is_err());
}

#[test]
fn recursive_membership_propagates_unreadable_descendants() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "").unwrap();
    std::fs::create_dir(temporary.path().join("broken-child")).unwrap();
    assert!(super::cgroup_processes(&File::open(temporary.path()).unwrap()).is_err());
}

#[test]
fn trusted_directory_walk_rejects_symlinks_and_writable_directories() {
    use std::os::unix::fs::PermissionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let root = File::open(temporary.path()).unwrap();
    std::fs::create_dir(temporary.path().join("writable")).unwrap();
    std::fs::set_permissions(
        temporary.path().join("writable"),
        std::fs::Permissions::from_mode(0o777),
    )
    .unwrap();
    assert!(super::open_trusted_directory(&root, std::path::Path::new("writable")).is_err());
    std::os::unix::fs::symlink("writable", temporary.path().join("link")).unwrap();
    assert!(super::open_trusted_directory(&root, std::path::Path::new("link")).is_err());
    assert!(super::open_trusted_directory(&root, std::path::Path::new("../outside")).is_err());
}

#[test]
fn termination_refuses_own_process_before_using_cgroup_kill() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir(temporary.path().join("nested")).unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "").unwrap();
    std::fs::write(
        temporary.path().join("nested/cgroup.procs"),
        format!("{}\n", std::process::id()),
    )
    .unwrap();
    std::fs::write(temporary.path().join("cgroup.kill"), "untouched").unwrap();
    let error = super::terminate_cgroup(
        &File::open(temporary.path()).unwrap(),
        std::time::Duration::ZERO,
    )
    .unwrap_err();
    assert!(error.to_string().contains("cleanup process"));
    assert_eq!(
        std::fs::read_to_string(temporary.path().join("cgroup.kill")).unwrap(),
        "untouched"
    );
}

#[test]
fn termination_reports_timeout_and_kill_errors() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "2147483647\n").unwrap();
    std::fs::write(temporary.path().join("cgroup.events"), "populated 1\n").unwrap();
    let directory = File::open(temporary.path()).unwrap();
    let error = super::terminate_cgroup(&directory, std::time::Duration::ZERO).unwrap_err();
    assert!(error.to_string().contains("timed out"));
    std::os::unix::fs::symlink("/dev/full", temporary.path().join("cgroup.kill")).unwrap();
    let error = super::terminate_cgroup(&directory, std::time::Duration::from_secs(1)).unwrap_err();
    assert!(error.to_string().contains("failed to kill shared cgroup"));
}

#[test]
fn fallback_signals_nested_member_through_pidfd_and_waits_for_empty() {
    use std::{process::Command, time::Duration};
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir(temporary.path().join("nested")).unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "").unwrap();
    std::fs::write(temporary.path().join("cgroup.events"), "populated 1\n").unwrap();
    let mut member = Command::new("sleep").arg("30").spawn().unwrap();
    let mut outside = Command::new("sleep").arg("30").spawn().unwrap();
    let membership = temporary.path().join("nested/cgroup.procs");
    std::fs::write(&membership, format!("{}\n", member.id())).unwrap();
    let result = std::thread::scope(|scope| {
        let watcher = scope.spawn(|| {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                if let Some(status) = member.try_wait().unwrap() {
                    std::fs::write(&membership, "").unwrap();
                    std::fs::write(temporary.path().join("cgroup.events"), "populated 0\n")
                        .unwrap();
                    return Some(status);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            None
        });
        let result = super::terminate_cgroup(
            &File::open(temporary.path()).unwrap(),
            Duration::from_secs(2),
        );
        (result, watcher.join().unwrap())
    });
    let outside_survived = outside.try_wait().unwrap().is_none();
    let _ = member.kill();
    let _ = member.wait();
    let _ = outside.kill();
    let _ = outside.wait();
    result.0.unwrap();
    assert!(result.1.is_some_and(|status| !status.success()));
    assert!(outside_survived);
}

#[test]
fn shared_removal_uses_captured_directories_after_mount_path_moves() {
    let temporary = tempfile::tempdir().unwrap();
    let original = temporary.path().join("original");
    std::fs::create_dir_all(original.join("demo/services/nested")).unwrap();
    let mut cgroup = Cgroup {
        paths: Vec::new(),
        unified: true,
        shared: Some(super::SharedCgroup {
            directory: File::open(original.join("demo")).unwrap(),
            parent: File::open(&original).unwrap(),
            name: "demo".to_owned(),
        }),
    };
    let captured = cgroup.directory().unwrap();
    let renamed = temporary.path().join("renamed");
    std::fs::rename(original, &renamed).unwrap();
    assert_eq!(
        captured.metadata().unwrap().ino(),
        cgroup.directory().unwrap().metadata().unwrap().ino()
    );
    cgroup.remove().unwrap();
    assert!(!renamed.join("demo").exists());
}

#[test]
fn termination_requires_populated_confirmation_even_for_empty_membership() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "").unwrap();
    let directory = File::open(temporary.path()).unwrap();
    assert!(super::terminate_cgroup(&directory, std::time::Duration::ZERO).is_err());
    std::fs::write(temporary.path().join("cgroup.events"), "populated 0\n").unwrap();
    super::terminate_cgroup(&directory, std::time::Duration::ZERO).unwrap();
    std::fs::write(temporary.path().join("cgroup.events"), "populated 1\n").unwrap();
    assert!(super::terminate_cgroup(&directory, std::time::Duration::ZERO).is_err());
}

#[test]
fn recursive_membership_preserves_permission_denied() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    const CHILD: &str = "KURUMI_CGROUP_PERMISSION_TEST";
    if let Some(path) = std::env::var_os(CHILD) {
        if kurumi_containerd_helper::process::effective_uid() == 0 {
            kurumi_containerd_helper::process::set_groups(&[]).unwrap();
            kurumi_containerd_helper::process::set_gid(65534).unwrap();
            kurumi_containerd_helper::process::set_uid(65534).unwrap();
        }
        let directory = File::open(path).unwrap();
        let error = super::cgroup_processes(&directory).unwrap_err();
        assert!(
            matches!(error, crate::RuntimeError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied)
        );
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "").unwrap();
    std::fs::create_dir(temporary.path().join("nested")).unwrap();
    let blocked = temporary.path().join("nested/cgroup.procs");
    std::fs::write(&blocked, "41\n").unwrap();
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o0)).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cgroup::tests::recursive_membership_preserves_permission_denied",
        ])
        .env(CHILD, temporary.path())
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn shared_creation_rejects_names_that_escape_dedicated_parent() {
    for name in ["", ".", "..", "/absolute", "nested/name", "name/"] {
        let result = Cgroup::create_shared(
            std::path::Path::new("/unused"),
            name,
            &ResourceConfig::default(),
        );
        assert!(
            result.is_err_and(|error| error.to_string().contains("invalid shared cgroup name")),
            "accepted {name:?}"
        );
    }
}

#[test]
fn recursive_scan_tolerates_a_captured_descendant_removed_midwalk() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "").unwrap();
    let child = temporary.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(child.join("cgroup.procs"), "42\n").unwrap();
    let mut visited = false;
    super::visit_cgroup_children(&File::open(temporary.path()).unwrap(), |captured, _| {
        std::fs::remove_file(child.join("cgroup.procs"))?;
        std::fs::remove_dir(&child)?;
        let mut processes = std::collections::BTreeSet::new();
        super::collect_cgroup_processes(captured, &mut processes)?;
        assert!(processes.is_empty());
        visited = true;
        Ok(())
    })
    .unwrap();
    assert!(visited);
}

#[test]
fn execution_leaf_rejects_symlink_escape() {
    let temporary = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("cgroup.procs"), "untouched").unwrap();
    std::os::unix::fs::symlink(outside.path(), temporary.path().join("kurumi-exec")).unwrap();
    let membership = CgroupMembership {
        directories: vec![File::open(temporary.path()).unwrap()],
    };
    assert!(membership.attach(42).is_err());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("cgroup.procs")).unwrap(),
        "untouched"
    );
}

#[test]
fn recovery_removal_refuses_members_and_preserves_siblings() {
    let temporary = tempfile::tempdir().unwrap();
    let container = temporary.path().join("demo");
    let sibling = temporary.path().join("other");
    std::fs::create_dir(&container).unwrap();
    std::fs::create_dir(&sibling).unwrap();
    std::fs::write(container.join("cgroup.procs"), "41\n").unwrap();
    std::fs::write(container.join("cgroup.events"), "populated 1\n").unwrap();
    let captured = File::open(&container).unwrap();
    assert!(
        super::remove_cgroup(&captured)
            .unwrap_err()
            .to_string()
            .contains("populated")
    );
    assert!(container.is_dir());
    assert!(sibling.is_dir());
    std::fs::remove_file(container.join("cgroup.procs")).unwrap();
    std::fs::remove_file(container.join("cgroup.events")).unwrap();
    std::fs::remove_dir(&container).unwrap();
    super::remove_cgroup(&captured).unwrap();
    assert!(sibling.is_dir());
}

#[test]
fn termination_does_not_fall_back_on_kill_permission_denied() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    const CHILD: &str = "KURUMI_CGROUP_KILL_PERMISSION_TEST";
    if let Some(path) = std::env::var_os(CHILD) {
        if kurumi_containerd_helper::process::effective_uid() == 0 {
            kurumi_containerd_helper::process::set_groups(&[]).unwrap();
            kurumi_containerd_helper::process::set_gid(65534).unwrap();
            kurumi_containerd_helper::process::set_uid(65534).unwrap();
        }
        let error = super::terminate_cgroup(
            &File::open(path).unwrap(),
            std::time::Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(error.to_string().contains("failed to kill shared cgroup"));
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(temporary.path().join("cgroup.procs"), "2147483647\n").unwrap();
    std::fs::write(temporary.path().join("cgroup.events"), "populated 1\n").unwrap();
    let kill = temporary.path().join("cgroup.kill");
    std::fs::write(&kill, "untouched").unwrap();
    std::fs::set_permissions(&kill, std::fs::Permissions::from_mode(0o444)).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cgroup::tests::termination_does_not_fall_back_on_kill_permission_denied",
        ])
        .env(CHILD, temporary.path())
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(std::fs::read_to_string(kill).unwrap(), "untouched");
}
