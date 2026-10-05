use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use crate::{Result, RuntimeError, error::bail};
use kurumi_containerd_config::Config;
use kurumi_containerd_helper::{
    fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW, OPEN_NONBLOCK},
    signal::{Signal, SignalNumber},
};
use serde::{Deserialize, Serialize};

use crate::host::process::ProcessHandle;

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
    if systemd_present(rootfs) {
        return InitSystem::Systemd;
    }
    for (system, paths) in [
        (InitSystem::Procd, &["sbin/procd", "usr/sbin/procd"][..]),
        (
            InitSystem::Openrc,
            &["sbin/openrc-init", "usr/bin/openrc-init", "sbin/openrc"][..],
        ),
        (InitSystem::Runit, &["sbin/runit", "usr/bin/runit"][..]),
        (InitSystem::S6, &["bin/s6-svscan", "usr/bin/s6-svscan"][..]),
    ] {
        if paths.iter().any(|path| rootfs.join(path).is_file()) {
            return system;
        }
    }

    let init = rootfs.join("sbin/init");
    if let Ok(target) = fs::read_link(&init) {
        let target = target.to_string_lossy();
        if target.contains("busybox") {
            return InitSystem::Busybox;
        }
        if target.contains("sysvinit") || target.contains("init.sysv") {
            return InitSystem::Sysvinit;
        }
        return InitSystem::Unknown;
    }
    if init.is_file() {
        let content = read_prefix(&init, 4096).unwrap_or_default();
        if content.windows(7).any(|value| value == b"systemd") {
            return InitSystem::Systemd;
        }
        if content.windows(7).any(|value| value == b"busybox") {
            return InitSystem::Busybox;
        }
        if content.windows(10).any(|value| value == b"/nix/store") {
            return InitSystem::Unknown;
        }
        return InitSystem::Sysvinit;
    }
    InitSystem::Unknown
}

pub(crate) fn request_shutdown(process: &ProcessHandle, system: InitSystem) -> Result<()> {
    match system {
        InitSystem::Systemd => process
            .send_signal(SignalNumber::realtime(3).expect("systemd shutdown signal must be valid")),
        InitSystem::Procd | InitSystem::S6 | InitSystem::Busybox => {
            process.send_signal(Signal::User2)
        }
        InitSystem::Runit => process.send_signal(Signal::Continue),
        InitSystem::Openrc => process.send_signal(Signal::Power),
        InitSystem::Sysvinit => request_sysv_shutdown(process),
        InitSystem::Custom => process.send_signal(Signal::Kill),
        InitSystem::Unknown => process.send_signal(Signal::Terminate),
    }
}

fn systemd_present(rootfs: &Path) -> bool {
    [
        "lib/systemd/systemd",
        "usr/lib/systemd/systemd",
        "bin/systemd",
        "usr/bin/systemd",
    ]
    .into_iter()
    .any(|path| rootfs.join(path).is_file())
        || fs::read_link(rootfs.join("sbin/init"))
            .is_ok_and(|target| target.to_string_lossy().contains("systemd"))
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
mod tests;
