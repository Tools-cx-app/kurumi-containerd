// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::fs;
use std::sync::{Arc, OnceLock};

use kurumi_containerd_helper::process::current_pid;
use procfs::process::Process;
use serde::{Deserialize, Serialize};

use super::super::process::{host_boot_id, process_start_time};
use crate::{Result, memory::MemoryFile};

/// Shared NAT ownership for all monitors belonging to one daemon.
#[derive(Clone)]
pub struct NetworkState {
    records: Arc<MemoryFile>,
    operations: Arc<MemoryFile>,
}

impl NetworkState {
    /// Creates empty, anonymous network ownership state.
    /// # Errors
    /// Returns an error when anonymous memory cannot be allocated.
    pub fn new() -> Result<Self> {
        Ok(Self {
            records: Arc::new(MemoryFile::new("kurumi-network")?),
            operations: Arc::new(MemoryFile::new("kurumi-network-operations")?),
        })
    }

    pub(super) fn lock(&self) -> Result<std::fs::File> {
        Ok(self.operations.lock()?)
    }

    pub(super) fn global() -> Result<Self> {
        static STATE: OnceLock<NetworkState> = OnceLock::new();
        if let Some(state) = STATE.get() {
            return Ok(state.clone());
        }
        let state = Self::new()?;
        Ok(STATE.get_or_init(|| state).clone())
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(super) struct NatLease {
    #[serde(default)]
    pub(super) owners: Vec<NatLeaseOwner>,
    pub(super) restore_disabled: bool,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct NatLeaseOwner {
    pub(super) host_boot_id: String,
    pub(super) pid: i32,
    pub(super) start_time: u64,
}

impl NatLeaseOwner {
    pub(super) fn current() -> Result<Self> {
        let pid = current_pid();
        Ok(Self {
            host_boot_id: host_boot_id()?,
            pid,
            start_time: process_start_time(pid)?,
        })
    }

    pub(super) fn is_live(&self, current_boot_id: &str) -> bool {
        self.host_boot_id == current_boot_id
            && Process::new(self.pid).is_ok_and(|process| {
                process
                    .stat()
                    .is_ok_and(|stat| stat.starttime == self.start_time && stat.state != 'Z')
            })
    }
}

pub(super) fn acquire_nat_lease(state: &NetworkState) -> Result<()> {
    state.records.update(|lease: &mut NatLease| {
        let owner = NatLeaseOwner::current()?;
        lease
            .owners
            .retain(|candidate| candidate.is_live(&owner.host_boot_id));
        if lease.owners.is_empty() {
            lease.restore_disabled =
                fs::read_to_string("/proc/sys/net/ipv4/ip_forward")?.trim() == "0";
        }
        if !lease.owners.contains(&owner) {
            lease.owners.push(owner);
        }
        fs::write("/proc/sys/net/ipv4/ip_forward", "1")?;
        Ok(())
    })
}

pub(super) fn release_nat_lease(state: &NetworkState) -> Result<()> {
    state.records.update(|lease: &mut NatLease| {
        let owner = NatLeaseOwner::current()?;
        lease
            .owners
            .retain(|candidate| candidate != &owner && candidate.is_live(&owner.host_boot_id));
        if lease.owners.is_empty() && lease.restore_disabled {
            fs::write("/proc/sys/net/ipv4/ip_forward", "0")?;
            lease.restore_disabled = false;
        }
        Ok(())
    })
}
