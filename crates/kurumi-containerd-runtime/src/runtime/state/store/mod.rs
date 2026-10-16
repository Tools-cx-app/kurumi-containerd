use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use fs2::FileExt;

use crate::{Result, error::ErrorContext as _};
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
        write_state_atomic(&self.recovery_path(state.uuid), state)?;
        write_state_atomic(&self.state_path_for(&state.name), state)
    }

    pub(crate) fn remove_state(&self) -> Result<()> {
        remove_if_exists(&self.state_path())
    }
    pub(crate) fn remove_state_for(&self, state: &ContainerState) -> Result<()> {
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

    fn open_lock(&self) -> Result<File> {
        Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(OPEN_NOFOLLOW | OPEN_CLOEXEC)
            .open(&self.lock_path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_state_and_recovery_paths_from_workdir_and_name() {
        let store = StateStore::new(Path::new("/run/kurumi"), "demo");
        assert_eq!(store.state_dir(), Path::new("/run/kurumi/state"));
        assert_eq!(store.recovery_dir(), Path::new("/run/kurumi/recovery"));
        assert_eq!(store.state_path(), Path::new("/run/kurumi/state/demo.json"));
    }
}
