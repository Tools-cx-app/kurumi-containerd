// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    Result, RuntimeError,
    error::{ErrorContext as _, bail, ensure},
    runtime_workdir,
};
use kurumi_containerd_config::IsolationMode;
#[cfg(test)]
use kurumi_containerd_helper::process::parent_pid as current_parent_pid;
use kurumi_containerd_helper::{
    fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW},
    process::effective_uid,
};
use procfs::process::{Process, all_processes};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Runtime,
    container::init::InitSystem,
    host::process::{host_boot_id, parent_pid, process_start_time, require_handle},
};

mod info;
pub(crate) mod store;

pub use info::ContainerInfo;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerState {
    pub name: String,
    pub init_pid: i32,
    pub monitor_pid: i32,
    pub rootfs: PathBuf,
    pub uuid: Uuid,
    pub host_boot_id: String,
    pub init_start_time: u64,
    #[serde(default)]
    pub pid_namespace_inode: Option<u64>,
    pub started_at_unix: u64,
    pub monitor_start_time: u64,
    #[serde(default)]
    pub init_system: InitSystem,
    #[serde(default)]
    pub generation: u64,
    #[serde(default)]
    pub isolation: IsolationMode,
    #[serde(default)]
    pub worker_pid: Option<i32>,
    #[serde(default)]
    pub worker_start_time: Option<u64>,
    #[serde(default)]
    pub mount_namespace_inode: Option<u64>,
    #[serde(default)]
    pub cgroup_path: Option<String>,
}

impl Runtime {
    /// Lists live containers tracked by the configured work directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the state directory cannot be read.
    pub fn list(&self) -> Result<Vec<ContainerState>> {
        self.store.list()
    }

    /// Scans the runtime work directory for live containers backed by trusted recovery metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when the recovery directory or state cannot be read or written safely.
    pub fn scan_all() -> Result<Vec<ContainerState>> {
        let store = store::StateStore::new(&runtime_workdir()?, "scan");
        Self::scan_with_store(&store)
    }

    /// Scans procfs for live containers backed by trusted recovery metadata and
    /// reconstructs missing state files.
    ///
    /// # Errors
    ///
    /// Returns an error when the recovery directory or state cannot be read or
    /// written safely.
    pub fn scan(&self) -> Result<Vec<ContainerState>> {
        Self::scan_with_store(&self.store)
    }

    fn scan_with_store(store: &store::StateStore) -> Result<Vec<ContainerState>> {
        Self::ensure_root()?;
        store.ensure_layout()?;
        let mut candidates: BTreeMap<String, (ContainerState, ContainerState)> = BTreeMap::new();
        for entry in fs::read_dir(store.recovery_dir())? {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(original) = read_state(&entry.path()) else {
                continue;
            };
            if !valid_state_name(&original.name)
                || entry.path() != store.recovery_path(original.uuid)
            {
                continue;
            }
            let mut state = original.clone();
            #[cfg(feature = "servicectl")]
            if state.isolation == IsolationMode::SharedPidIpc {
                // Shared generations never replace their recorded supervisor with a service.
                if !shared_metadata_valid(&state)
                    || host_boot_id().ok().as_deref() != Some(&state.host_boot_id)
                {
                    continue;
                }
                if !validate_process_identity(&state)
                    && shared_cgroup_directory(&state).ok().flatten().is_none()
                {
                    // Keep residual metadata even when supervision cannot be authenticated.
                    continue;
                }
            }
            if state.isolation == IsolationMode::Isolated {
                if host_boot_id().ok().as_deref() != Some(&state.host_boot_id)
                    || !validate_monitor_identity(&state)
                {
                    store.remove_recovery_if_unchanged(&original)?;
                    continue;
                }
                let Some(init_pid) = find_generation_init(&state)? else {
                    continue;
                };
                let handle = require_handle(init_pid)?;
                state.init_pid = init_pid;
                state.init_start_time = process_start_time(init_pid)?;
                state.pid_namespace_inode = Some(namespace_inode(init_pid, "pid")?);
                if handle.pid().as_raw() != init_pid || !validate_process_identity(&state) {
                    continue;
                }
            }
            if let Some((_, existing)) = candidates.get(&state.name) {
                ensure!(
                    existing.uuid == state.uuid,
                    "multiple live recovery records claim container '{}'",
                    state.name
                );
            }
            candidates.insert(state.name.clone(), (original, state));
        }
        let mut recovered = Vec::new();
        for (original, state) in candidates.into_values() {
            if store.restore_state_if_unchanged(&original, &state)? {
                recovered.push(state);
            }
        }
        Ok(recovered)
    }

    pub(crate) fn ensure_root() -> Result<()> {
        ensure!(
            effective_uid() == 0,
            "this operation requires root privileges"
        );
        Ok(())
    }

    pub(crate) fn ensure_layout(&self) -> Result<()> {
        self.store.ensure_layout()
    }

    fn state_path(&self) -> PathBuf {
        self.store.state_path()
    }

    pub(crate) fn state(&self) -> Result<Option<ContainerState>> {
        self.store.state()
    }

    pub(crate) fn require_state(&self) -> Result<ContainerState> {
        let state = self.state()?.with_context(|| {
            format!("container '{}' is not running", self.config.container.name)
        })?;
        ensure!(
            validate_process_identity(&state),
            "container supervisor identity is unavailable; stop the residual generation before starting or executing"
        );
        Ok(state)
    }

    pub(crate) fn state_for_stop(&self) -> Result<ContainerState> {
        let path = self.state_path();
        let state = read_state(&path).with_context(|| {
            format!("container '{}' is not running", self.config.container.name)
        })?;
        ensure!(
            state.name == self.config.container.name && state_is_present(&state),
            "container '{}' is not running",
            self.config.container.name
        );
        Ok(state)
    }

    pub(crate) fn write_state(&self, state: &ContainerState) -> Result<()> {
        self.store.write_state(state)
    }

    pub(crate) fn remove_state_for(&self, state: &ContainerState) -> Result<()> {
        self.store.remove_state_for(state)
    }

    pub(crate) fn remove_state(&self) -> Result<()> {
        self.store.remove_state()
    }

    pub(crate) fn lock(&self) -> Result<File> {
        self.store.lock()
    }

    pub(crate) fn wait_for_monitor_cleanup(&self, state: &ContainerState) -> Result<()> {
        #[cfg(feature = "servicectl")]
        let shared_directory = if state.isolation == IsolationMode::SharedPidIpc {
            shared_cgroup_directory(state)?
        } else {
            None
        };
        let monitor = match require_handle(state.monitor_pid) {
            Ok(monitor) => monitor,
            Err(_) if !validate_monitor_identity(state) => {
                #[cfg(feature = "servicectl")]
                ensure_shared_empty(shared_directory.as_ref())?;
                self.remove_state_for(state)?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        ensure!(
            validate_monitor_identity(state),
            "container monitor identity changed before pidfd validation"
        );
        if monitor.wait_for_exit(Duration::from_secs(5))? {
            #[cfg(feature = "servicectl")]
            ensure_shared_empty(shared_directory.as_ref())?;
            self.remove_state_for(state)?;
            return Ok(());
        }
        bail!("container init exited but monitor did not finish cleanup")
    }
}

pub(crate) fn validate_process_identity(state: &ContainerState) -> bool {
    Process::new(state.init_pid).is_ok_and(|process| validate_process_identity_for(state, &process))
}

pub(crate) fn validate_process_identity_for(state: &ContainerState, process: &Process) -> bool {
    if process.pid != state.init_pid
        || !host_boot_id().is_ok_and(|value| value == state.host_boot_id)
        || !process
            .stat()
            .is_ok_and(|stat| stat.starttime == state.init_start_time && stat.state != 'Z')
        || !process_marker_matches(process, "name", &state.name)
    {
        return false;
    }
    match state.isolation {
        IsolationMode::Isolated => state
            .pid_namespace_inode
            .is_some_and(|expected| process_namespace_inode(process, "pid").ok() == Some(expected)),
        #[cfg(feature = "servicectl")]
        IsolationMode::SharedPidIpc => {
            shared_metadata_valid(state)
                && process_marker_matches(process, "uuid", &state.uuid.to_string())
                && process_namespace_inode(process, "mnt").ok() == state.mount_namespace_inode
                && state
                    .worker_pid
                    .and_then(|pid| Process::new(pid).ok())
                    .is_some_and(|worker| {
                        process_namespace_inode(&worker, "mnt")
                            .is_ok_and(|inode| Some(inode) != state.mount_namespace_inode)
                    })
                && shared_membership_matches(state, process)
                && validate_worker_identity(state)
                && process
                    .stat()
                    .is_ok_and(|stat| Some(stat.ppid) == state.worker_pid)
        }
    }
}

fn process_marker_matches(process: &Process, marker: &str, expected: &str) -> bool {
    process
        .open_relative(format!("root/run/kurumi-containerd/{marker}"))
        .and_then(|mut file| {
            use std::io::Read;
            let mut value = String::new();
            file.read_to_string(&mut value)?;
            Ok(value)
        })
        .is_ok_and(|value| value.trim() == expected)
}

#[cfg(feature = "servicectl")]
fn shared_metadata_valid(state: &ContainerState) -> bool {
    state.init_pid > 0
        && state.monitor_pid > 0
        && state.init_pid != state.monitor_pid
        && state
            .worker_pid
            .is_some_and(|pid| pid > 0 && pid != state.init_pid && pid != state.monitor_pid)
        && state.worker_start_time.is_some_and(|time| time > 0)
        && state.init_start_time > 0
        && state.monitor_start_time > 0
        && state.mount_namespace_inode.is_some_and(|inode| inode > 0)
        && valid_state_name(&state.name)
        && state.cgroup_path.as_deref().is_some_and(|value| {
            let path = Path::new(value);
            path.is_absolute()
                && value
                    .split('/')
                    .skip(1)
                    .all(|part| !part.is_empty() && part != "." && part != "..")
                && path
                    .file_name()
                    .is_some_and(|name| name == state.name.as_str())
                && path
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "kurumi-containerd")
        })
}

#[cfg(feature = "servicectl")]
fn shared_membership_matches(state: &ContainerState, process: &Process) -> bool {
    state.cgroup_path.as_deref().is_some_and(|root| {
        process.cgroups().is_ok_and(|groups| {
            groups.0.iter().any(|group| {
                group.controllers.is_empty()
                    && Path::new(&group.pathname).strip_prefix(root).is_ok()
            })
        })
    })
}

#[cfg(feature = "servicectl")]
fn validate_worker_identity(state: &ContainerState) -> bool {
    state
        .worker_pid
        .and_then(|pid| Process::new(pid).ok())
        .is_some_and(|worker| {
            host_boot_id().is_ok_and(|boot| boot == state.host_boot_id)
                && worker.stat().is_ok_and(|stat| {
                    Some(stat.starttime) == state.worker_start_time
                        && stat.state != 'Z'
                        && (!validate_monitor_identity(state) || stat.ppid == state.monitor_pid)
                })
                && (!validate_monitor_identity(state)
                    || Process::new(state.monitor_pid).is_ok_and(|monitor| {
                        process_namespace_inode(&worker, "mnt")
                            .ok()
                            .zip(process_namespace_inode(&monitor, "mnt").ok())
                            .is_some_and(|(worker, monitor)| worker == monitor)
                    }))
        })
}

fn state_is_present(state: &ContainerState) -> bool {
    #[cfg(feature = "servicectl")]
    if state.isolation == IsolationMode::SharedPidIpc {
        // A dead supervisor is not proof that its cgroup has finished cleanup.
        return shared_metadata_valid(state)
            && host_boot_id().is_ok_and(|boot| boot == state.host_boot_id);
    }
    validate_process_identity(state) || validate_monitor_identity(state)
}

#[cfg(feature = "servicectl")]
pub(crate) fn shared_cgroup_directory(state: &ContainerState) -> Result<Option<File>> {
    use crate::host::cgroup::Cgroup;
    ensure!(
        state.isolation == IsolationMode::SharedPidIpc && shared_metadata_valid(state),
        "shared generation metadata is incomplete"
    );
    ensure!(
        host_boot_id()? == state.host_boot_id,
        "shared generation belongs to a previous host boot"
    );
    let anchor = if validate_monitor_identity(state) {
        state.monitor_pid
    } else {
        ensure!(
            validate_worker_identity(state),
            "shared generation has no trusted live monitor or worker; preserving residual state"
        );
        state
            .worker_pid
            .context("shared generation has no worker PID")?
    };
    let process = Process::new(anchor)?;
    let captured = Cgroup::capture_shared(
        &process,
        state
            .cgroup_path
            .as_deref()
            .context("missing shared cgroup path")?,
    );
    ensure!(
        if anchor == state.monitor_pid {
            validate_monitor_identity(state)
        } else {
            validate_worker_identity(state)
        },
        "shared cgroup anchor identity changed during capture"
    );
    match captured {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if shared_cgroup_missing(&error) && !validate_process_identity(state) => {
            Ok(None)
        }
        Err(error) => Err(error)
            .context("failed to capture shared generation cgroup; preserving residual state"),
    }
}

#[cfg(feature = "servicectl")]
fn shared_cgroup_missing(error: &RuntimeError) -> bool {
    let mut source: &(dyn std::error::Error + 'static) = error;
    loop {
        if source
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            || matches!(
                source.downcast_ref::<procfs::ProcError>(),
                Some(procfs::ProcError::NotFound(_))
            )
        {
            return true;
        }
        let Some(next) = source.source() else {
            return false;
        };
        source = next;
    }
}

#[cfg(feature = "servicectl")]
fn ensure_shared_empty(directory: Option<&File>) -> Result<()> {
    if let Some(directory) = directory {
        ensure!(
            crate::host::cgroup::cgroup_processes(directory)?.is_empty(),
            "shared generation still has cgroup members; preserving residual state"
        );
    }
    Ok(())
}

fn validate_monitor_identity(state: &ContainerState) -> bool {
    Process::new(state.monitor_pid)
        .is_ok_and(|process| validate_monitor_identity_for(state, &process))
}

pub(crate) fn validate_monitor_identity_for(state: &ContainerState, process: &Process) -> bool {
    host_boot_id().is_ok_and(|value| value == state.host_boot_id)
        && process.pid == state.monitor_pid
        && process
            .stat()
            .is_ok_and(|stat| stat.starttime == state.monitor_start_time && stat.state != 'Z')
}

pub(crate) fn namespace_inode(pid: i32, namespace: &str) -> Result<u64> {
    process_namespace_inode(&Process::new(pid)?, namespace)
}

fn process_namespace_inode(process: &Process, namespace: &str) -> Result<u64> {
    process
        .namespaces()?
        .0
        .get(std::ffi::OsStr::new(namespace))
        .map(|value| value.identifier)
        .with_context(|| format!("process {} has no {namespace} namespace", process.pid))
}

fn read_state(path: &Path) -> Result<ContainerState> {
    use std::os::unix::fs::MetadataExt;

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_NOFOLLOW | OPEN_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    let parent = fs::metadata(path.parent().context("state path has no parent")?)?;
    ensure!(metadata.is_file(), "state path is not a regular file");
    ensure!(
        metadata.uid() == parent.uid(),
        "state file owner does not match its directory"
    );
    ensure!(
        metadata.mode() & 0o022 == 0,
        "state file must not be group/world writable"
    );
    let state: ContainerState = serde_json::from_reader(file)?;
    ensure!(
        match state.isolation {
            IsolationMode::Isolated => state.pid_namespace_inode.is_some_and(|inode| inode > 0),
            #[cfg(feature = "servicectl")]
            IsolationMode::SharedPidIpc => shared_metadata_valid(&state),
        },
        "container state has incomplete isolation identity metadata"
    );
    Ok(state)
}

fn write_state_atomic(path: &Path, state: &ContainerState) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(OPEN_NOFOLLOW | OPEN_CLOEXEC)
        .open(&temporary)?;
    let result = (|| {
        serde_json::to_writer_pretty(&file, state)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(path.parent().context("state path has no parent")?)?.sync_all()?;
        Ok::<(), RuntimeError>(())
    })();
    if result.is_err() {
        fs::remove_file(temporary).ok();
    }
    result
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn find_generation_init(expected: &ContainerState) -> Result<Option<i32>> {
    for process in all_processes()? {
        let Ok(process) = process else {
            continue;
        };
        let pid = process.pid;
        if namespace_pid(&process).ok().flatten() != Some(1) {
            continue;
        }
        let root = PathBuf::from(format!("/proc/{pid}/root/run/kurumi-containerd"));
        if fs::read_to_string(root.join("name")).ok().as_deref() != Some(&expected.name)
            || fs::read_to_string(root.join("uuid"))
                .ok()
                .is_none_or(|value| value.trim() != expected.uuid.to_string())
        {
            continue;
        }
        let Ok(intermediate) = parent_pid(pid) else {
            continue;
        };
        if intermediate <= 0 || parent_pid(intermediate).ok() != Some(expected.monitor_pid) {
            continue;
        }
        return Ok(Some(pid));
    }
    Ok(None)
}

fn valid_state_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn namespace_pid(process: &Process) -> Result<Option<i32>> {
    Ok(process
        .status()?
        .nspid
        .and_then(|values| values.last().copied()))
}

fn ensure_trusted_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if !path.exists() {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir(),
        "runtime directory is not a directory: {}",
        path.display()
    );
    ensure!(
        !metadata.file_type().is_symlink(),
        "runtime directory must not be a symlink: {}",
        path.display()
    );
    ensure!(
        metadata.uid() == 0,
        "runtime directory must be owned by root: {}",
        path.display()
    );
    ensure!(
        metadata.mode() & 0o022 == 0,
        "runtime directory must not be group/world writable: {}",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/state/tests.rs"]
mod tests;
