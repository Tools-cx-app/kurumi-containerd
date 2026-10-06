//! Core container lifecycle and host-integration primitives for `KurumiContainerd`.

mod container;
use kurumi_containerd_error as error;
use kurumi_containerd_host as host;
mod runtime;

use std::path::{Path, PathBuf};

use kurumi_containerd_config::Config;

#[cfg(target_os = "android")]
use crate::error::ErrorContext as _;
pub use crate::error::{Result, RuntimeError};
use crate::{container::init::Init, host::rootfs::Rootfs};
pub use crate::{
    container::init::InitSystem,
    runtime::state::{ContainerInfo, ContainerState},
};

/// Public operations supported by a configured container runtime.
pub trait ContainerRuntime {
    /// Installs a rootfs archive into the configured target.
    ///
    /// # Errors
    /// Returns an error when installation or target validation fails.
    fn install(&self, archive: &Path, size: Option<u64>, force: bool) -> Result<()>;
    /// Starts the configured container.
    ///
    /// # Errors
    /// Returns an error when startup or container setup fails.
    fn start(&self, foreground_override: bool) -> Result<ContainerState>;
    /// Stops the configured container.
    ///
    /// # Errors
    /// Returns an error when shutdown or cleanup fails.
    fn stop(&self) -> Result<()>;
    /// Restarts the configured container.
    ///
    /// # Errors
    /// Returns an error when stopping or starting fails.
    fn restart(&self, foreground: bool) -> Result<ContainerState>;
    /// Opens an interactive login session inside the container.
    ///
    /// # Errors
    /// Returns an error when the container or login session is unavailable.
    fn enter(&self, user: &str) -> Result<i32>;
    /// Runs a command inside the container.
    ///
    /// # Errors
    /// Returns an error when namespace entry or command execution fails.
    fn run(&self, command: &[String]) -> Result<i32>;
    /// Returns container status and resource usage.
    ///
    /// # Errors
    /// Returns an error when state or resource information cannot be read.
    fn info(&self) -> Result<ContainerInfo>;
    /// Returns the validated init PID.
    ///
    /// # Errors
    /// Returns an error when the container is not running.
    fn pid(&self) -> Result<i32>;
    /// Lists live containers tracked by the runtime.
    ///
    /// # Errors
    /// Returns an error when runtime state cannot be read.
    fn list(&self) -> Result<Vec<ContainerState>>;
    /// Recovers live containers with missing state files.
    ///
    /// # Errors
    /// Returns an error when recovery metadata cannot be read or written.
    fn scan(&self) -> Result<Vec<ContainerState>>;
}

/// A configured container runtime.
pub struct Runtime {
    pub(crate) config: Config,
    pub(crate) init: Init,
    pub(crate) rootfs: Rootfs,
    pub(crate) workdir: PathBuf,
    pub(crate) state_dir: PathBuf,
    pub(crate) recovery_dir: PathBuf,
    pub(crate) volatile_dir: PathBuf,
    pub(crate) lock_path: PathBuf,
}

impl Runtime {
    /// Creates a runtime for one validated container configuration.
    ///
    /// # Errors
    ///
    /// Returns an error on Android when `TMP` is not set.
    pub fn new(config: Config) -> Result<Self> {
        let workdir = runtime_workdir()?;
        let state_dir = workdir.join("state");
        let recovery_dir = workdir.join("recovery");
        let mount_dir = workdir.join("mounts");
        let volatile_dir = workdir.join("volatile");
        let lock_path = workdir.join(format!("{}.lock", config.container.name));
        let init = Init::new(&config);
        let rootfs = Rootfs::new(&config, &mount_dir);
        Ok(Self {
            config,
            init,
            rootfs,
            workdir,
            state_dir,
            recovery_dir,
            volatile_dir,
            lock_path,
        })
    }
}

impl ContainerRuntime for Runtime {
    fn install(&self, archive: &Path, size: Option<u64>, force: bool) -> Result<()> {
        Runtime::install(self, archive, size, force)
    }

    fn start(&self, foreground_override: bool) -> Result<ContainerState> {
        Runtime::start(self, foreground_override)
    }

    fn stop(&self) -> Result<()> {
        Runtime::stop(self)
    }

    fn restart(&self, foreground: bool) -> Result<ContainerState> {
        Runtime::restart(self, foreground)
    }

    fn enter(&self, user: &str) -> Result<i32> {
        Runtime::enter(self, user)
    }

    fn run(&self, command: &[String]) -> Result<i32> {
        Runtime::run(self, command)
    }

    fn info(&self) -> Result<ContainerInfo> {
        Runtime::info(self)
    }

    fn pid(&self) -> Result<i32> {
        Runtime::pid(self)
    }

    fn list(&self) -> Result<Vec<ContainerState>> {
        Runtime::list(self)
    }

    fn scan(&self) -> Result<Vec<ContainerState>> {
        Runtime::scan(self)
    }
}

#[allow(clippy::unnecessary_wraps)]
pub(crate) fn runtime_workdir() -> Result<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let tmp = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .context("TMPDIR is not set")?;
        error::ensure!(tmp.is_absolute(), "TMPDIR must be an absolute path");
        Ok(tmp.join("kurumi-containerd"))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(PathBuf::from("/run/kurumi-containerd"))
    }
}

/// Reports whether the current kernel exposes pidfd process handles.
#[must_use]
pub fn pidfd_available() -> bool {
    host::process::ProcessHandle::open(kurumi_containerd_helper::process::current_pid()).is_ok()
}
