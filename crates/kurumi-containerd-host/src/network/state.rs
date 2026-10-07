use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

use fs2::FileExt;
use kurumi_containerd_helper::fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW};
use procfs::process::Process;
use serde::{Deserialize, Serialize};

use crate::{Result, error::ensure};

use super::super::process::{host_boot_id, process_start_time};
use kurumi_containerd_helper::process::current_pid;

pub(super) fn network_lock(state_dir: &Path) -> Result<File> {
    let state_dir = network_state_dir(state_dir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(OPEN_NOFOLLOW | OPEN_CLOEXEC)
        .open(state_dir.join("network.lock"))?;
    file.lock_exclusive()?;
    Ok(file)
}

pub(super) fn network_state_dir(path: &Path) -> Result<&Path> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(metadata.is_dir(), "network state path is not a directory");
    ensure!(
        !metadata.file_type().is_symlink(),
        "network state directory must not be a symlink"
    );
    ensure!(
        metadata.uid() == 0,
        "network state directory must be owned by root"
    );
    ensure!(
        metadata.mode() & 0o022 == 0,
        "network state directory must not be group or world writable"
    );
    Ok(path)
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

pub(super) fn acquire_nat_lease(state_dir: &Path) -> Result<()> {
    let path = network_state_dir(state_dir)?.join("network-state.json");
    let mut lease = read_nat_lease(&path);
    let owner = NatLeaseOwner::current()?;
    lease
        .owners
        .retain(|candidate| candidate.is_live(&owner.host_boot_id));
    if lease.owners.is_empty() && !path.exists() {
        lease.restore_disabled = fs::read_to_string("/proc/sys/net/ipv4/ip_forward")?.trim() == "0";
    }
    if !lease.owners.contains(&owner) {
        lease.owners.push(owner);
    }
    write_nat_lease(&path, &lease)
}

pub(super) fn release_nat_lease(state_dir: &Path) -> Result<()> {
    let path = network_state_dir(state_dir)?.join("network-state.json");
    let mut lease = read_nat_lease(&path);
    let owner = NatLeaseOwner::current()?;
    lease
        .owners
        .retain(|candidate| candidate != &owner && candidate.is_live(&owner.host_boot_id));
    if lease.owners.is_empty() {
        if lease.restore_disabled {
            fs::write("/proc/sys/net/ipv4/ip_forward", "0")?;
        }
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        return Ok(());
    }
    write_nat_lease(&path, &lease)
}

fn read_nat_lease(path: &Path) -> NatLease {
    fs::read(path)
        .ok()
        .and_then(|source| serde_json::from_slice(&source).ok())
        .unwrap_or_default()
}

fn write_nat_lease(path: &Path, lease: &NatLease) -> Result<()> {
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec(lease)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}
