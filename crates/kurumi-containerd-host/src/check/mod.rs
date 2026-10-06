use std::{os::unix::fs::PermissionsExt, path::Path};

use crate::{Result, process::ProcessHandle};
use kurumi_containerd_helper::process::{
    ForkResult, NamespaceFlags, WaitStatus, fork, unshare, waitpid,
};

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
        self.capabilities
            .iter()
            .find(|capability| capability.name == "Namespaces")
            .is_some_and(|capability| capability.available)
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
        let namespaces = [
            probe_namespace(NamespaceFlags::MOUNT),
            probe_namespace(NamespaceFlags::PID),
            probe_namespace(NamespaceFlags::UTS),
            probe_namespace(NamespaceFlags::IPC),
            probe_namespace(NamespaceFlags::NETWORK),
        ];
        let namespaces_available = namespaces.into_iter().all(|available| available);
        let mountinfo = procfs::process::Process::myself()?.mountinfo()?;
        let capabilities = vec![
            capability(
                "Namespaces",
                namespaces_available,
                "mount, pid, uts, ipc, network",
            ),
            capability(
                "OverlayFS",
                std::fs::read_to_string("/proc/filesystems")?.contains("overlay"),
                "kernel filesystem",
            ),
            capability(
                "Cgroup v2",
                Path::new("/sys/fs/cgroup/cgroup.controllers").exists(),
                "unified hierarchy",
            ),
            capability(
                "Cgroup v1",
                mountinfo.0.iter().any(|mount| mount.fs_type == "cgroup"),
                "legacy hierarchy",
            ),
            capability(
                "Pidfd",
                ProcessHandle::open(kurumi_containerd_helper::process::current_pid()).is_ok(),
                "process handles",
            ),
            command_capability("ip"),
            command_capability("iptables"),
        ];
        Ok(HostCheckReport { capabilities })
    }
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

#[allow(unsafe_code)]
fn probe_namespace(flag: NamespaceFlags) -> bool {
    match unsafe { fork() } {
        Err(_) => false,
        Ok(ForkResult::Child) => {
            let code = i32::from(unshare(flag).is_err());
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
mod tests;
