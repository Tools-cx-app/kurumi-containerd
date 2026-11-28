// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

//! Core container lifecycle and host-integration primitives for `KurumiContainerd`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kurumi_containerd_config::Config;
use kurumi_containerd_error as error;
use kurumi_containerd_host as host;

#[cfg(target_os = "android")]
use crate::error::ErrorContext as _;
use crate::{container::init::Init, host::rootfs::Rootfs, runtime::state::store::StateStore};
pub use crate::{
    container::init::InitSystem,
    error::{Result, RuntimeError},
    runtime::{
        query::ContainerQuery,
        state::{ContainerInfo, ContainerState},
    },
};

mod container;
mod runtime;

/// Public operations supported by a configured container runtime.
pub trait ContainerRuntime {
    /// Installs a rootfs archive into the configured target.
    ///
    /// # Errors
    /// Returns an error when installation or target validation fails.
    fn install(&self, archive: &Path, size: Option<u64>, force: bool) -> Result<()>;
    /// Exports the stopped container's rootfs to a new local archive.
    ///
    /// # Errors
    /// Returns an error when the container is running or export fails.
    fn backup(&self, output: &Path) -> Result<()>;
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
    /// An explicit shell command starts with an absolute container path and bypasses `su`.
    ///
    /// # Errors
    /// Returns an error when the container or login session is unavailable.
    fn enter(&self, user: &str, shell: Option<&str>) -> Result<i32>;
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
    /// Validates tracked live containers.
    ///
    /// # Errors
    /// Returns an error when memory-backed state cannot be read.
    fn scan(&self) -> Result<Vec<ContainerState>>;
}

/// Owns the fork-shared container records for one daemon instance.
#[derive(Default)]
pub struct RuntimeRegistry {
    stores: BTreeMap<String, StateStore>,
    network: Option<host::network::NetworkState>,
    #[cfg(target_os = "android")]
    selinux: Option<host::android::SelinuxState>,
}

impl RuntimeRegistry {
    /// Opens a container runtime using the existing record for its name, if any.
    ///
    /// # Errors
    /// Returns an error when runtime memory or the working directory is unavailable.
    pub fn open(&mut self, config: Config) -> Result<Runtime> {
        let store = match self.stores.entry(config.container.name.clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.get().clone(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(StateStore::new()?).clone()
            }
        };
        if self.network.is_none() {
            self.network = Some(host::network::NetworkState::new()?);
        }
        let mut runtime = Runtime::with_store(config, store)?;
        if let Some(network) = &self.network {
            runtime.network = network.clone();
        }
        #[cfg(target_os = "android")]
        {
            if self.selinux.is_none() {
                self.selinux = Some(host::android::SelinuxState::new()?);
            }
            if let Some(selinux) = &self.selinux {
                runtime.selinux = selinux.clone();
            }
        }
        runtime.owner = Some(host::process::ProcessHandle::open(
            kurumi_containerd_helper::process::current_pid(),
        )?);
        Ok(runtime)
    }

    /// Returns the currently tracked containers in name order.
    /// # Errors
    /// Returns an error when a record cannot be read.
    pub fn list(&self) -> Result<Vec<ContainerState>> {
        self.stores
            .values()
            .filter_map(|store| store.state().transpose())
            .collect()
    }
}

/// A configured container runtime.
pub struct Runtime {
    pub(crate) config: Config,
    pub(crate) init: Init,
    pub(crate) rootfs: Rootfs,
    pub(crate) workdir: PathBuf,
    pub(crate) store: StateStore,
    pub(crate) volatile_dir: PathBuf,
    pub(crate) network: host::network::NetworkState,
    #[cfg(target_os = "android")]
    pub(crate) selinux: host::android::SelinuxState,
    pub(crate) owner: Option<host::process::ProcessHandle>,
}

impl Runtime {
    /// Creates a runtime for one validated container configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when anonymous memory is unavailable or Android `TMPDIR` is not set.
    pub fn new(config: Config) -> Result<Self> {
        Self::with_store(config, StateStore::new()?)
    }

    fn with_store(config: Config, store: StateStore) -> Result<Self> {
        let workdir = runtime_workdir()?;
        let mount_dir = workdir.join("mounts");
        let volatile_dir = workdir.join("volatile");
        let init = Init::new(&config);
        let rootfs = Rootfs::from_paths(
            config.container.rootfs.as_deref(),
            config.container.rootfs_image.as_deref(),
            &mount_dir.join(&config.container.name),
        );
        Ok(Self {
            config,
            init,
            rootfs,
            workdir,
            store,
            volatile_dir,
            network: host::network::NetworkState::new()?,
            #[cfg(target_os = "android")]
            selinux: host::android::SelinuxState::new()?,
            owner: None,
        })
    }
}

impl ContainerRuntime for Runtime {
    fn install(&self, archive: &Path, size: Option<u64>, force: bool) -> Result<()> {
        Runtime::install(self, archive, size, force)
    }

    fn backup(&self, output: &Path) -> Result<()> {
        Runtime::backup(self, output)
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

    fn enter(&self, user: &str, shell: Option<&str>) -> Result<i32> {
        Runtime::enter(self, user, shell)
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
