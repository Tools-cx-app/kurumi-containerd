// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(target_os = "android")]
use std::sync::OnceLock;

use kurumi_containerd_config::AndroidConfig;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorContext as _, ensure};
use crate::{Result, memory::MemoryFile};

/// Shared `SELinux` ownership retained by the daemon and its monitors.
#[derive(Clone)]
pub struct SelinuxState(Arc<MemoryFile>);

impl SelinuxState {
    /// Creates anonymous `SELinux` ownership state.
    /// # Errors
    /// Returns an error when anonymous memory cannot be allocated.
    pub fn new() -> Result<Self> {
        Ok(Self(Arc::new(MemoryFile::new("kurumi-selinux")?)))
    }

    #[cfg(target_os = "android")]
    fn global() -> Result<Self> {
        static STATE: OnceLock<SelinuxState> = OnceLock::new();
        if let Some(state) = STATE.get() {
            return Ok(state.clone());
        }
        let state = Self::new()?;
        Ok(STATE.get_or_init(|| state).clone())
    }
}

pub struct SelinuxGuard {
    state: Option<SelinuxState>,
    enforce: PathBuf,
}

impl SelinuxGuard {
    /// Acquires shared `SELinux` ownership using a process-wide memory record.
    /// # Errors
    /// Returns errors when changing enforcement or accessing the memory record fails.
    #[cfg(target_os = "android")]
    pub fn apply(config: &AndroidConfig, _workdir: &Path) -> Result<Self> {
        Self::apply_in_memory(config, &SelinuxState::global()?)
    }

    /// Acquires `SELinux` ownership from the daemon's shared memory record.
    /// # Errors
    /// Returns errors when enforcement or memory access fails.
    #[cfg(target_os = "android")]
    pub fn apply_in_memory(config: &AndroidConfig, state: &SelinuxState) -> Result<Self> {
        Self::apply_at(config, state, Path::new("/sys/fs/selinux/enforce"))
    }

    fn apply_at(config: &AndroidConfig, state: &SelinuxState, enforce: &Path) -> Result<Self> {
        if !config.selinux_permissive {
            return Ok(Self {
                state: None,
                enforce: enforce.to_path_buf(),
            });
        }
        ensure!(enforce.exists(), "SELinux enforce control is unavailable");
        state.0.update(|lease: &mut Lease| {
            if lease.users == 0 {
                lease.restore_enforcing = fs::read_to_string(enforce)?.trim() == "1";
                if lease.restore_enforcing {
                    write_control(enforce, b"0")?;
                }
            }
            lease.users = lease
                .users
                .checked_add(1)
                .context("SELinux lease count overflow")?;
            Ok(())
        })?;
        Ok(Self {
            state: Some(state.clone()),
            enforce: enforce.to_path_buf(),
        })
    }

    fn release(&self, state: &SelinuxState) -> Result<()> {
        state.0.update(|lease: &mut Lease| {
            if lease.users == 1 && lease.restore_enforcing {
                write_control(&self.enforce, b"1")
                    .context("failed to restore SELinux enforcing mode")?;
                lease.restore_enforcing = false;
            }
            lease.users = lease.users.saturating_sub(1);
            Ok(())
        })
    }
}

impl Drop for SelinuxGuard {
    fn drop(&mut self) {
        if let Some(state) = self.state.take()
            && let Err(error) = self.release(&state)
        {
            tracing::error!(%error, "failed to release SELinux ownership");
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
struct Lease {
    users: u64,
    restore_enforcing: bool,
}

fn write_control(path: &Path, value: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(value)?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/selinux/tests.rs"]
mod tests;
