// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use kurumi_containerd_helper::fs::LoopController;
use kurumi_containerd_helper::process::{
    ForkResult, NamespaceFlags, WaitStatus, current_pid, fork, unshare, waitpid,
};
use kurumi_containerd_helper::security::{FilterInstruction, SECCOMP_ALLOW, install_seccomp};
use kurumi_containerd_helper::terminal::{open_pty, socket_pair};
use procfs::process::MountInfo;

use crate::{Result, error::ErrorContext as _, memory::MemoryFile, process::ProcessHandle};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    pub name: String,
    pub available: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostCheckReport {
    pub capabilities: Vec<Capability>,
}

impl HostCheckReport {
    #[must_use]
    pub fn required_namespaces_available(&self) -> bool {
        ["Mount namespace", "UTS namespace"].iter().all(|name| {
            self.capabilities
                .iter()
                .any(|capability| capability.name == *name && capability.available)
        })
    }
}

pub trait HostCheck {
    ///
    /// # Errors
    ///
    /// Returns an error when host capability information cannot be collected.
    fn check(&self) -> Result<HostCheckReport>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HostCapabilities;

impl HostCheck for HostCapabilities {
    fn check(&self) -> Result<HostCheckReport> {
        let filesystems = fs::read_to_string("/proc/filesystems")
            .context("failed to read /proc/filesystems (CONFIG_PROC_FS)")?;
        let mountinfo = procfs::process::Process::myself()?.mountinfo()?;
        let unified = mountinfo.0.iter().find(|mount| mount.fs_type == "cgroup2");
        let mut capabilities = isolation_capabilities(unified.is_some());
        capabilities.extend(filesystem_capabilities(&filesystems));
        capabilities.extend([
            capability(
                "Seccomp filter",
                probe_in_child(probe_seccomp_filter),
                "CONFIG_SECCOMP and CONFIG_SECCOMP_FILTER for all containers",
            ),
            capability(
                "Unix sockets",
                socket_pair().is_ok(),
                "CONFIG_UNIX for the daemon and PTY broker",
            ),
            capability(
                "Unix98 PTY",
                open_pty(None).is_ok(),
                "CONFIG_UNIX98_PTYS for the host console",
            ),
            capability(
                "memfd with flock",
                MemoryFile::new("kurumi-check")
                    .and_then(|file| file.lock())
                    .is_ok(),
                "memfd_create and CONFIG_FILE_LOCKING for runtime state",
            ),
            capability(
                "Cgroup v1",
                mountinfo.0.iter().any(|mount| mount.fs_type == "cgroup"),
                "mounted legacy hierarchy",
            ),
            capability(
                "Pidfd",
                ProcessHandle::open(current_pid())
                    .and_then(|handle| handle.wait_for_exit(Duration::ZERO))
                    .is_ok(),
                "pidfd_open and CONFIG_EPOLL for lifecycle operations",
            ),
            capability(
                "Loop control device",
                LoopController::open().is_ok(),
                "CONFIG_BLK_DEV_LOOP and /dev/loop-control for regular-file rootfs images",
            ),
            command_capability("ip"),
            command_capability("iptables"),
        ]);
        let controllers = unified.and_then(|mount| {
            fs::read_to_string(mount.mount_point.join("cgroup.controllers")).ok()
        });
        capabilities.extend(controller_capabilities(
            &mountinfo.0,
            controllers.as_deref(),
        ));
        Ok(HostCheckReport { capabilities })
    }
}

fn isolation_capabilities(unified: bool) -> Vec<Capability> {
    let mount = probe_namespace(NamespaceFlags::MOUNT);
    let uts = probe_namespace(NamespaceFlags::UTS);
    let pid = probe_namespace(NamespaceFlags::PID);
    let ipc = probe_namespace(NamespaceFlags::IPC);
    #[cfg(feature = "servicectl")]
    let cgroup = probe_namespace(NamespaceFlags::CGROUP);
    vec![
        capability(
            "Mount namespace",
            mount,
            "CONFIG_NAMESPACES for all containers",
        ),
        capability("UTS namespace", uts, "CONFIG_UTS_NS for all containers"),
        capability("PID namespace", pid, "CONFIG_PID_NS for isolated mode"),
        capability("IPC namespace", ipc, "CONFIG_IPC_NS for isolated mode"),
        capability(
            "Host network namespace handle",
            fs::File::open("/proc/self/ns/net").is_ok(),
            "monitor opens /proc/self/ns/net in every network mode",
        ),
        capability(
            "Network namespace",
            probe_namespace(NamespaceFlags::NETWORK),
            "CONFIG_NET_NS for non-host networking",
        ),
        capability(
            "User namespace",
            probe_namespace(NamespaceFlags::USER),
            "CONFIG_USER_NS for nested workloads with allow_user_namespaces",
        ),
        capability(
            "Isolated mode namespaces",
            mount && uts && pid && ipc,
            "resource and init requirements checked at start",
        ),
        capability("Cgroup v2", unified, "mounted unified hierarchy"),
        #[cfg(feature = "servicectl")]
        capability("Cgroup namespace", cgroup, "required by shared mode"),
        #[cfg(feature = "servicectl")]
        capability(
            "Shared PID/IPC namespaces",
            mount && uts && cgroup && unified,
            "also requires writable dedicated cgroup v2 and a foreground supervisor at start",
        ),
    ]
}

fn controller_capabilities(mounts: &[MountInfo], unified: Option<&str>) -> [Capability; 3] {
    [
        ("cpu", "CONFIG_CFS_BANDWIDTH for resources.cpu_quota"),
        ("memory", "CONFIG_MEMCG for resources.memory_bytes"),
        ("pids", "CONFIG_CGROUP_PIDS for resources.pids"),
    ]
    .map(|(controller, requirement)| {
        let on_v2 = unified.is_some_and(|names| {
            names
                .split_ascii_whitespace()
                .any(|name| name == controller)
        });
        let on_v1 = mounts
            .iter()
            .any(|mount| mount.fs_type == "cgroup" && mount.super_options.contains_key(controller));
        let source = if on_v2 {
            "advertised on v2"
        } else if on_v1 {
            "mounted on v1"
        } else {
            "not visible"
        };
        capability(
            &format!("Cgroup {controller} controller"),
            on_v1 || on_v2,
            &format!("{source}, {requirement}"),
        )
    })
}

fn filesystem_capabilities(filesystems: &str) -> impl Iterator<Item = Capability> {
    [
        ("proc", "procfs", "CONFIG_PROC_FS for /proc"),
        ("sysfs", "sysfs", "CONFIG_SYSFS for /sys"),
        (
            "tmpfs",
            "tmpfs",
            "CONFIG_TMPFS for /dev, /dev/shm, /run and /tmp",
        ),
        (
            "devpts",
            "devpts",
            "CONFIG_UNIX98_PTYS for private /dev/pts",
        ),
        (
            "overlay",
            "OverlayFS",
            "CONFIG_OVERLAY_FS for volatile rootfs",
        ),
        ("ext4", "ext4", "CONFIG_EXT4_FS for ext4 rootfs images"),
        ("btrfs", "btrfs", "CONFIG_BTRFS_FS for btrfs rootfs images"),
        (
            "binfmt_misc",
            "binfmt_misc",
            "CONFIG_BINFMT_MISC for foreign ELF interpreters",
        ),
    ]
    .into_iter()
    .map(|(fstype, name, detail)| {
        let registered = filesystems
            .lines()
            .any(|line| line.split_ascii_whitespace().next_back() == Some(fstype));
        capability(name, registered, detail)
    })
}

fn capability(name: &str, available: bool, detail: &str) -> Capability {
    Capability {
        name: name.to_owned(),
        available,
        detail: detail.to_owned(),
    }
}

fn command_capability(command: &str) -> Capability {
    let detail = command_path(command).map_or_else(
        || "not found in PATH".to_owned(),
        |path| path.display().to_string(),
    );
    capability(command, detail != "not found in PATH", &detail)
}

fn command_path(command: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(command))
        .find(|path| {
            path.is_file()
                && std::fs::metadata(path)
                    .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
        })
}

fn probe_namespace(flag: NamespaceFlags) -> bool {
    probe_in_child(|| unshare(flag))
}

fn probe_seccomp_filter() -> std::io::Result<()> {
    const BPF_RET_K: u16 = 0x06;
    install_seccomp(&mut [FilterInstruction {
        code: BPF_RET_K,
        jump_true: 0,
        jump_false: 0,
        value: SECCOMP_ALLOW,
    }])
}

/// Runs the syscall probe in a disposable child of the single-threaded host check.
fn probe_in_child(probe: impl FnOnce() -> std::io::Result<()>) -> bool {
    #[allow(unsafe_code)]
    let result = unsafe { fork() };
    match result {
        Err(_) => false,
        Ok(ForkResult::Child) => {
            let code = i32::from(probe().is_err());
            std::process::exit(code);
        }
        Ok(ForkResult::Parent { child }) => loop {
            match waitpid(child, false) {
                Ok(status) => break matches!(status, WaitStatus::Exited(_, 0)),
                Err(error) if kurumi_containerd_helper::process::is_interrupted(&error) => {}
                Err(_) => break false,
            }
        },
    }
}

#[cfg(test)]
#[path = "../tests/unit/check/tests.rs"]
mod tests;
