// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use kurumi_containerd_config::Config;
use kurumi_containerd_helper::{
    fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW, OPEN_NONBLOCK},
    process::{
        ForkResult, NamespaceFlags, WaitStatus, chdir, chroot, close_fds_except, execve, fchdir,
        fork, is_interrupted, setns, setsid, waitpid,
    },
    signal::{Signal, kill, kill_process_group},
};
use serde::{Deserialize, Serialize};

use crate::{
    Result, RuntimeError,
    error::{ErrorContext as _, bail, ensure},
    host::process::ProcessHandle,
};

#[derive(Debug, Clone)]
pub(crate) struct Init {
    path: PathBuf,
}

impl Init {
    pub(crate) fn new(config: &Config) -> Self {
        Self {
            path: config.container.init.clone(),
        }
    }

    pub(crate) fn prepare(&self, rootfs: &Path) -> Result<()> {
        let relative = self
            .path
            .strip_prefix("/")
            .map_err(|_| RuntimeError::Message("container.init must be an absolute path".into()))?;
        let init = kurumi_containerd_config::resolve_container_path(rootfs, relative)?;
        let metadata = init
            .symlink_metadata()
            .map_err(|error| RuntimeError::Context {
                context: format!("init does not exist: {}", init.display()),
                source: Box::new(error),
            })?;
        if !metadata.is_file() {
            bail!("init is not a regular file: {}", init.display());
        }
        if metadata.permissions().mode() & 0o111 == 0 {
            bail!("init is not executable: {}", init.display());
        }
        Ok(())
    }

    #[cfg(feature = "servicectl")]
    pub(crate) fn validate_shared(&self, rootfs: &Path) -> Result<()> {
        let relative = self
            .path
            .strip_prefix("/")
            .map_err(|_| RuntimeError::Message("container.init must be an absolute path".into()))?;
        let resolved = kurumi_containerd_config::resolve_container_path(rootfs, relative)?;
        let is_pid_one_program = |path: &Path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            matches!(
                name,
                "systemd" | "openrc-init" | "procd" | "runit" | "busybox" | "init.sysv"
            ) || (name == "init"
                && matches!(
                    path.parent().and_then(Path::to_str),
                    Some("/sbin" | "/usr/sbin" | "/bin" | "/usr/bin")
                ))
        };
        if is_pid_one_program(&self.path) || is_pid_one_program(&resolved) {
            bail!(
                "shared-pid-ipc requires a foreground service supervisor, not a PID 1 init: {}",
                self.path.display()
            );
        }
        Ok(())
    }

    pub(crate) fn detect(&self, rootfs: &Path) -> InitSystem {
        detect(rootfs, &self.path)
    }
}

pub(crate) fn prepare_runtime(system: InitSystem) -> Result<()> {
    if system != InitSystem::Systemd {
        return Ok(());
    }

    fs::create_dir_all("/run/systemd/journal")?;
    fs::create_dir_all("/run/systemd/system")?;
    fs::write("/run/systemd/container", "kurumi-containerd")?;
    std::os::unix::fs::symlink(
        "/dev/null",
        "/run/systemd/system/systemd-journald-audit.socket",
    )?;
    std::os::unix::fs::symlink(
        "/dev/null",
        "/run/systemd/system/systemd-networkd-wait-online.service",
    )?;
    fs::create_dir_all("/run/systemd/journald.conf.d")?;
    fs::write(
        "/run/systemd/journald.conf.d/kurumi-containerd.conf",
        "[Journal]\nReadKMsg=no\nAudit=no\nStorage=volatile\n",
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InitSystem {
    #[default]
    Unknown,
    Systemd,
    Procd,
    Openrc,
    Runit,
    S6,
    Busybox,
    Sysvinit,
    Custom,
}

impl std::fmt::Display for InitSystem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Unknown => "unknown",
            Self::Systemd => "systemd",
            Self::Procd => "procd",
            Self::Openrc => "openrc",
            Self::Runit => "runit",
            Self::S6 => "s6",
            Self::Busybox => "busybox",
            Self::Sysvinit => "sysvinit",
            Self::Custom => "custom",
        };
        formatter.write_str(name)
    }
}

pub(crate) fn detect(rootfs: &Path, configured_init: &Path) -> InitSystem {
    if configured_init != Path::new("/sbin/init") {
        return InitSystem::Custom;
    }
    let Ok(init) = kurumi_containerd_config::resolve_container_path(rootfs, configured_init) else {
        return InitSystem::Unknown;
    };
    if !init.is_file() {
        return InitSystem::Unknown;
    }
    let system = match init.file_name().and_then(|name| name.to_str()) {
        Some("systemd") => Some(InitSystem::Systemd),
        Some("procd") => Some(InitSystem::Procd),
        Some("openrc-init") => Some(InitSystem::Openrc),
        Some("runit") => Some(InitSystem::Runit),
        Some("s6-svscan") => Some(InitSystem::S6),
        Some("busybox") => Some(InitSystem::Busybox),
        Some("init.sysv" | "sysvinit") => Some(InitSystem::Sysvinit),
        _ => None,
    };
    if let Some(system) = system {
        return system;
    }
    let content = read_prefix(&init, 4096).unwrap_or_default();
    if content.windows(7).any(|value| value == b"systemd") {
        return InitSystem::Systemd;
    }
    if content.windows(7).any(|value| value == b"busybox") {
        return InitSystem::Busybox;
    }
    if content.windows(10).any(|value| value == b"/nix/store")
        || init.file_name().is_none_or(|name| name != "init")
    {
        return InitSystem::Unknown;
    }
    InitSystem::Sysvinit
}

pub(crate) fn request_shutdown(process: &ProcessHandle, system: InitSystem) -> Result<()> {
    match system {
        InitSystem::Systemd => request_systemd_shutdown(process),
        InitSystem::Procd | InitSystem::S6 | InitSystem::Busybox => {
            process.send_signal(Signal::User2)
        }
        InitSystem::Runit => process.send_signal(Signal::Continue),
        InitSystem::Openrc => process.send_signal(Signal::Power),
        InitSystem::Sysvinit => request_sysv_shutdown(process),
        InitSystem::Custom | InitSystem::Unknown => process.send_signal(Signal::Terminate),
    }
}

/// Uses the container's control client so host and guest libc signal bases may differ.
fn request_systemd_shutdown(process: &ProcessHandle) -> Result<()> {
    ensure!(
        procfs::process::Process::new(process.pid().as_raw())?
            .status()?
            .nspid
            .is_some_and(|pids| pids.last() == Some(&1)),
        "systemd shutdown target is not a namespace init"
    );
    let root_path = PathBuf::from(format!("/proc/{}/root", process.pid().as_raw()));
    let root = File::open(&root_path).context("failed to open systemd container root")?;
    // Join PID as well as mount namespaces: the guest /proc must resolve this
    // client's /proc/self correctly, including on kernels without close_range.
    let namespaces = ["mnt", "pid"]
        .map(|name| -> Result<_> {
            let target = File::open(format!("/proc/{}/ns/{name}", process.pid().as_raw()))?;
            let current = fs::metadata(format!("/proc/self/ns/{name}"))?;
            let metadata = target.metadata()?;
            Ok(
                ((metadata.dev(), metadata.ino()) != (current.dev(), current.ino()))
                    .then_some(target),
            )
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        namespaces.iter().all(Option::is_some),
        "systemd shutdown requires private mount and PID namespaces"
    );
    ensure!(
        !process.wait_for_exit(Duration::ZERO)?,
        "systemd init exited during shutdown resource capture"
    );
    let executable = ["/usr/bin/systemctl", "/bin/systemctl"]
        .into_iter()
        .find(|path| {
            kurumi_containerd_config::resolve_container_path(&root_path, Path::new(path))
                .is_ok_and(|resolved| resolved.is_file())
        })
        .context("systemd shutdown requires systemctl inside the container")?;
    // Request the unit directly through the private bus, avoiding signal-based
    // fallbacks in systemctl's halt verb.
    let arguments = [
        executable,
        "--no-block",
        "--job-mode=replace-irreversibly",
        "start",
        "halt.target",
    ]
    .map(|value| CString::new(value).expect("static systemctl argument contains no NUL"));
    // Only the guest /run is visible after chroot. Explicitly permit the live
    // operation there; systemctl otherwise treats a chroot as an offline rootfs.
    let environment = [
        "PATH=/usr/sbin:/usr/bin:/sbin:/bin",
        "SYSTEMD_IGNORE_CHROOT=1",
    ]
    .map(|value| CString::new(value).expect("static environment contains no NUL"));
    // SAFETY: shutdown runs in the single-threaded CLI or monitor. The child
    // only joins namespaces, forks the client, closes descriptors and execs or exits.
    let worker = {
        #[allow(unsafe_code)]
        unsafe {
            fork()
        }
    }
    .context("failed to fork systemd shutdown client")?;
    match worker {
        ForkResult::Parent { child } => wait_shutdown_client(child, Duration::from_secs(5)),
        ForkResult::Child => {
            let result = (|| -> Result<()> {
                setsid()?;
                for namespace in namespaces.iter().flatten() {
                    setns(namespace.as_fd(), NamespaceFlags::EMPTY)?;
                }
                // SAFETY: this worker is single-threaded; its child enters the
                // selected PID namespace before executing the control client.
                let client = {
                    #[allow(unsafe_code)]
                    unsafe {
                        fork()
                    }
                }?;
                match client {
                    ForkResult::Parent { child } => loop {
                        match waitpid(child, false) {
                            Ok(WaitStatus::Exited(_, code)) => std::process::exit(code),
                            Err(error) if is_interrupted(&error) => {}
                            _ => std::process::exit(126),
                        }
                    },
                    ForkResult::Child => {
                        // SAFETY: closed inherited owners are never used or
                        // dropped; all subsequent paths exec or exit directly.
                        #[allow(unsafe_code)]
                        unsafe { close_fds_except(&[root.as_fd()]) }?;
                        fchdir(&root)?;
                        chroot(".")?;
                        chdir("/")?;
                        execve(&arguments[0], &arguments, &environment)?;
                        Ok(())
                    }
                }
            })();
            if let Err(error) = result {
                tracing::error!(%error, "systemd shutdown client failed");
            }
            std::process::exit(126);
        }
    }
}

fn wait_shutdown_client(child: i32, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match waitpid(child, true) {
            Ok(WaitStatus::StillAlive) => {}
            Ok(status) => {
                ensure!(
                    matches!(status, WaitStatus::Exited(_, 0)),
                    "systemd shutdown client failed: {status:?}"
                );
                return Ok(());
            }
            Err(error) if is_interrupted(&error) => continue,
            Err(error) => return Err(error).context("failed waiting for systemd shutdown client"),
        }
        if Instant::now() >= deadline {
            // The unreaped child cannot have its PID recycled.
            let _ = kill_process_group(child, Signal::Kill);
            let _ = kill(child, Signal::Kill);
            loop {
                match waitpid(child, false) {
                    Err(error) if is_interrupted(&error) => {}
                    _ => break,
                }
            }
            bail!("systemd shutdown client timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn request_sysv_shutdown(process: &ProcessHandle) -> Result<()> {
    let request = init_request();
    for path in ["run/initctl", "dev/initctl"] {
        let target = PathBuf::from(format!("/proc/{}/root/{path}", process.pid().as_raw()));
        if let Ok(mut file) = OpenOptions::new()
            .write(true)
            .custom_flags(OPEN_NONBLOCK | OPEN_CLOEXEC | OPEN_NOFOLLOW)
            .open(&target)
            && file.write_all(&request).is_ok()
        {
            return Ok(());
        }
    }
    process.send_signal(Signal::Power)
}

fn init_request() -> [u8; 384] {
    let mut request = [0_u8; 384];
    for (offset, value) in [(0, 0x0309_1969_i32), (4, 1), (8, i32::from(b'0')), (12, 3)] {
        request[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
    }
    request
}

fn read_prefix(path: &Path, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut content = Vec::new();
    fs::File::open(path)?
        .take(limit as u64)
        .read_to_end(&mut content)?;
    Ok(content)
}

#[cfg(test)]
#[path = "../../tests/unit/container/init/tests.rs"]
mod tests;
