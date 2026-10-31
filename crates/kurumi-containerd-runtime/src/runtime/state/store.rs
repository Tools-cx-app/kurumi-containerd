// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use fs2::FileExt;

use crate::{Result, RuntimeError, error::ErrorContext as _};
use kurumi_containerd_helper::fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW};

use super::{
    ContainerState, ensure_trusted_directory, read_state, remove_if_exists,
    validate_monitor_identity, validate_process_identity, write_state_atomic,
};

pub(crate) struct StateStore {
    workdir: PathBuf,
    state_dir: PathBuf,
    recovery_dir: PathBuf,
    lock_path: PathBuf,
    name: String,
}

impl StateStore {
    pub(crate) fn new(workdir: &Path, name: &str) -> Self {
        Self {
            workdir: workdir.to_path_buf(),
            state_dir: workdir.join("state"),
            recovery_dir: workdir.join("recovery"),
            lock_path: workdir.join(format!("{name}.lock")),
            name: name.to_owned(),
        }
    }

    pub(crate) fn ensure_layout(&self) -> Result<()> {
        ensure_trusted_directory(&self.workdir)?;
        fs::create_dir_all(&self.state_dir)
            .with_context(|| format!("failed to create workdir {}", self.workdir.display()))?;
        ensure_trusted_directory(&self.state_dir)?;
        fs::create_dir_all(&self.recovery_dir)?;
        ensure_trusted_directory(&self.recovery_dir)
    }

    #[cfg(test)]
    pub(crate) fn state_dir(&self) -> &Path {
        &self.state_dir
    }
    pub(crate) fn recovery_dir(&self) -> &Path {
        &self.recovery_dir
    }
    pub(crate) fn state_path(&self) -> PathBuf {
        self.state_dir.join(format!("{}.json", self.name))
    }
    pub(crate) fn state_path_for(&self, name: &str) -> PathBuf {
        self.state_dir.join(format!("{name}.json"))
    }
    pub(crate) fn recovery_path(&self, uuid: uuid::Uuid) -> PathBuf {
        self.recovery_dir.join(format!("{uuid}.json"))
    }

    pub(crate) fn list(&self) -> Result<Vec<ContainerState>> {
        self.ensure_layout()?;
        let mut states = Vec::new();
        for entry in fs::read_dir(&self.state_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            if let Ok(state) = read_state(&entry.path())
                && (validate_process_identity(&state) || validate_monitor_identity(&state))
            {
                states.push(state);
            }
        }
        states.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(states)
    }

    pub(crate) fn state(&self) -> Result<Option<ContainerState>> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(None);
        }
        let state = read_state(&path)?;
        Ok(
            (validate_process_identity(&state) || validate_monitor_identity(&state))
                .then_some(state),
        )
    }

    pub(crate) fn write_state(&self, state: &ContainerState) -> Result<()> {
        let _lock = self.lock_updates(&state.name)?;
        self.write_state_locked(state)
    }

    fn write_state_locked(&self, state: &ContainerState) -> Result<()> {
        write_state_atomic(&self.recovery_path(state.uuid), state)?;
        write_state_atomic(&self.state_path_for(&state.name), state)
    }

    pub(crate) fn restore_state_if_unchanged(
        &self,
        original: &ContainerState,
        recovered: &ContainerState,
    ) -> Result<bool> {
        let _lock = self.lock_updates(&original.name)?;
        if read_optional_state(&self.recovery_path(original.uuid))?.as_ref() != Some(original) {
            return Ok(false);
        }
        // A different record for this name may have been published under a new UUID.
        if read_optional_state(&self.state_path_for(&original.name))?
            .is_some_and(|current| current != *original && current != *recovered)
        {
            return Ok(false);
        }
        self.write_state_locked(recovered)?;
        Ok(true)
    }

    pub(crate) fn remove_recovery_if_unchanged(&self, original: &ContainerState) -> Result<()> {
        let _lock = self.lock_updates(&original.name)?;
        let path = self.recovery_path(original.uuid);
        if read_optional_state(&path)?.as_ref() == Some(original) {
            remove_if_exists(&path)?;
        }
        Ok(())
    }

    pub(crate) fn remove_state(&self) -> Result<()> {
        let _lock = self.lock_updates(&self.name)?;
        remove_if_exists(&self.state_path())
    }
    pub(crate) fn remove_state_for(&self, state: &ContainerState) -> Result<()> {
        let _lock = self.lock_updates(&self.name)?;
        remove_if_exists(&self.state_path())?;
        remove_if_exists(&self.recovery_path(state.uuid))
    }

    pub(crate) fn lock(&self) -> Result<File> {
        let file = self.open_lock()?;
        file.lock_exclusive()?;
        Ok(file)
    }

    pub(crate) fn try_lock(&self) -> Result<Option<File>> {
        let file = self.open_lock()?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error).context("failed to try-lock container lifecycle"),
        }
    }

    pub(crate) fn request_stop(&self) -> Result<File> {
        let file = self.open_stop_lock()?;
        file.lock_exclusive()?;
        Ok(file)
    }

    pub(crate) fn stop_requested(&self) -> Result<bool> {
        let file = self.open_stop_lock()?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
            Err(error) => Err(error).context("failed to check container stop request"),
        }
    }

    fn open_stop_lock(&self) -> Result<File> {
        // A held descriptor represents a pending stop, so crashes leave no stale request.
        Self::open_lock_file(&self.state_dir.join(format!("{}.stop", self.name)))
    }

    fn open_lock(&self) -> Result<File> {
        Self::open_lock_file(&self.lock_path)
    }

    fn lock_updates(&self, name: &str) -> Result<File> {
        // Recovery can run while install/backup holds the lifecycle lock. Keep
        // persistence transactions separate, and never acquire lifecycle locks here.
        let file = Self::open_lock_file(&self.state_dir.join(format!("{name}.lock")))?;
        file.lock_exclusive()?;
        Ok(file)
    }

    fn open_lock_file(path: &Path) -> Result<File> {
        Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(OPEN_NOFOLLOW | OPEN_CLOEXEC)
            .open(path)?)
    }
}

fn read_optional_state(path: &Path) -> Result<Option<ContainerState>> {
    match read_state(path) {
        Ok(state) => Ok(Some(state)),
        Err(RuntimeError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/state/store/tests.rs"]
mod tests;
