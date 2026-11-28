// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::fs::File;
use std::io::{Read, Seek, Write};
use std::sync::Arc;

use crate::host::memory::MemoryFile;
use crate::{ContainerState, Result};

use super::state_is_present;

/// The daemon retains one store per container name. Forked workers and monitors
/// use the same anonymous descriptors to publish and read the current generation.
#[derive(Clone)]
pub(crate) struct StateStore(Arc<Files>);

struct Files {
    records: MemoryFile,
    lifecycle: MemoryFile,
    stop: MemoryFile,
}

impl StateStore {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self(Arc::new(Files {
            records: MemoryFile::new("kurumi-state")?,
            lifecycle: MemoryFile::new("kurumi-lifecycle")?,
            stop: MemoryFile::new("kurumi-stop")?,
        })))
    }

    pub(crate) fn state(&self) -> Result<Option<ContainerState>> {
        let state = read_record(&mut self.0.records.lock()?)?;
        Ok(state.filter(state_is_present))
    }

    pub(crate) fn write_state(&self, state: &ContainerState) -> Result<()> {
        let payload = serde_json::to_vec(state)?;
        let mut file = self.0.records.lock()?;
        // Commit into the inactive slot before switching the single-byte selector.
        let slot = if active_slot(&mut file)? == 1 { 2 } else { 1 };
        if payload.len() > SLOT_SIZE - 4 {
            return Err(std::io::Error::other("container state exceeds slot capacity").into());
        }
        file.seek(std::io::SeekFrom::Start(slot_offset(slot)))?;
        file.write_all(
            &u32::try_from(payload.len())
                .expect("payload fits in slot")
                .to_ne_bytes(),
        )?;
        file.write_all(&payload)?;
        file.rewind()?;
        file.write_all(&[slot])?;
        Ok(())
    }

    pub(crate) fn remove_state_for(&self, state: &ContainerState) -> Result<()> {
        self.remove_matching(|current| current == state)?;
        Ok(())
    }

    pub(crate) fn remove_state_for_monitor(&self, pid: i32) -> Result<()> {
        self.remove_matching(|current| current.monitor_pid == pid)?;
        Ok(())
    }

    #[cfg(feature = "servicectl")]
    pub(crate) fn remove_shared_state_for_worker(&self, pid: i32, start_time: u64) -> Result<bool> {
        let boot_id = crate::host::process::host_boot_id()?;
        self.remove_matching(|current| {
            current.isolation == kurumi_containerd_config::IsolationMode::SharedPidIpc
                && current.worker_pid == Some(pid)
                && current.worker_start_time == Some(start_time)
                && current.host_boot_id == boot_id
        })
    }

    fn remove_matching(&self, matches: impl FnOnce(&ContainerState) -> bool) -> Result<bool> {
        let mut file = self.0.records.lock()?;
        if !read_record(&mut file)?.as_ref().is_some_and(matches) {
            return Ok(false);
        }
        file.set_len(0)?;
        Ok(true)
    }

    pub(crate) fn lock(&self) -> Result<File> {
        Ok(self.0.lifecycle.lock()?)
    }

    pub(crate) fn try_lock(&self) -> Result<Option<File>> {
        Ok(self.0.lifecycle.try_lock()?)
    }

    pub(crate) fn request_stop(&self) -> Result<File> {
        Ok(self.0.stop.lock()?)
    }

    pub(crate) fn stop_requested(&self) -> Result<bool> {
        Ok(self.0.stop.try_lock()?.is_none())
    }
}

const SLOT_SIZE: usize = 64 * 1024;

fn slot_offset(slot: u8) -> u64 {
    1 + u64::from(slot - 1) * SLOT_SIZE as u64
}

fn active_slot(file: &mut File) -> std::io::Result<u8> {
    let mut slot = [0];
    match file.read(&mut slot)? {
        0 => return Ok(0),
        1 => {}
        _ => unreachable!("one-byte buffer"),
    }
    match slot[0] {
        0..=2 => Ok(slot[0]),
        _ => Err(std::io::Error::other("invalid in-memory state slot")),
    }
}

fn read_record(file: &mut File) -> Result<Option<ContainerState>> {
    let slot = active_slot(file)?;
    if slot == 0 {
        return Ok(None);
    }
    file.seek(std::io::SeekFrom::Start(slot_offset(slot)))?;
    let mut length = [0; 4];
    file.read_exact(&mut length)?;
    let length = u32::from_ne_bytes(length) as usize;
    if length > SLOT_SIZE - 4 {
        return Err(std::io::Error::other("invalid in-memory state length").into());
    }
    let mut payload = vec![0; length];
    file.read_exact(&mut payload)?;
    Ok(Some(serde_json::from_slice(&payload)?))
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/state/store/tests.rs"]
mod tests;
