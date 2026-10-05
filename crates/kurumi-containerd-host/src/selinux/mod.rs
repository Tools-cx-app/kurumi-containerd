use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use crate::{
    Result,
    error::{ErrorContext as _, ensure},
};
use fs2::FileExt;
use kurumi_containerd_config::AndroidConfig;
use kurumi_containerd_helper::fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW};
use serde::{Deserialize, Serialize};

pub struct SelinuxGuard {
    workdir: Option<PathBuf>,
    enforce: PathBuf,
}

impl SelinuxGuard {
    /// Acquires the requested shared `SELinux` permissive state.
    ///
    /// # Errors
    /// Returns errors when accessing `SELinux` controls or lease state fails.
    pub fn apply(config: &AndroidConfig, workdir: &Path) -> Result<Self> {
        Self::apply_at(config, workdir, Path::new("/sys/fs/selinux/enforce"))
    }

    fn apply_at(config: &AndroidConfig, workdir: &Path, enforce: &Path) -> Result<Self> {
        if !config.selinux_permissive {
            return Ok(Self {
                workdir: None,
                enforce: enforce.to_path_buf(),
            });
        }
        ensure!(enforce.exists(), "SELinux enforce control is unavailable");
        let _lock = selinux_lock(workdir)?;
        let state_path = workdir.join("selinux-state.json");
        let mut state = read_selinux_state(&state_path)?;
        if state.users == 0 {
            state.restore_enforcing = fs::read_to_string(enforce)?.trim() == "1";
        }
        let changed_enforcement = state.users == 0 && state.restore_enforcing;
        state.users = state
            .users
            .checked_add(1)
            .context("SELinux lease count overflow")?;
        if changed_enforcement {
            write_control(enforce, b"0")?;
        }
        if let Err(error) = write_selinux_state(&state_path, &state) {
            if changed_enforcement && let Err(rollback) = write_control(enforce, b"1") {
                return Err(error).with_context(|| format!(
                    "failed to commit SELinux lease; restoring enforcing mode also failed: {rollback}"
                ));
            }
            return Err(error).context("failed to commit SELinux lease");
        }
        Ok(Self {
            workdir: Some(workdir.to_path_buf()),
            enforce: enforce.to_path_buf(),
        })
    }

    fn release(&self, workdir: &Path) -> Result<()> {
        let _lock = selinux_lock(workdir)?;
        let state_path = workdir.join("selinux-state.json");
        let mut state = read_selinux_state(&state_path)?;
        state.users = state.users.saturating_sub(1);
        if state.users == 0 {
            if state.restore_enforcing {
                write_control(&self.enforce, b"1")
                    .context("failed to restore SELinux enforcing mode")?;
            }
            match fs::remove_file(state_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        } else {
            write_selinux_state(&state_path, &state)
        }
    }
}

impl Drop for SelinuxGuard {
    fn drop(&mut self) {
        let Some(workdir) = self.workdir.take() else {
            return;
        };
        if let Err(error) = self.release(&workdir) {
            tracing::error!(%error, "failed to release SELinux lease; recovery state retained");
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct SelinuxState {
    users: u64,
    restore_enforcing: bool,
}

fn write_control(path: &Path, value: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(value)?;
    Ok(())
}

fn selinux_lock(workdir: &Path) -> Result<File> {
    fs::create_dir_all(workdir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(OPEN_NOFOLLOW | OPEN_CLOEXEC)
        .open(workdir.join("selinux.lock"))?;
    file.lock_exclusive()?;
    Ok(file)
}

fn read_selinux_state(path: &Path) -> Result<SelinuxState> {
    match fs::read(path) {
        Ok(source) => {
            serde_json::from_slice(&source).context("failed to parse SELinux lease state")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SelinuxState::default()),
        Err(error) => Err(error).context("failed to read SELinux lease state"),
    }
}

fn write_selinux_state(path: &Path, state: &SelinuxState) -> Result<()> {
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec(state)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
