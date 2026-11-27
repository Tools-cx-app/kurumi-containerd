// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

//! Lifetime management of kernel `binfmt_misc` rules for foreign ELF executables.

use std::{
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use kurumi_containerd_config::{BinfmtArch, BinfmtConfig};
use kurumi_containerd_helper::fs::{MountFlags, mount, unmount};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    Result,
    error::{ErrorContext as _, ensure},
};

mod elf;

/// Owns exactly one rule and a private mount of the shared `binfmt_misc` registry.
///
/// The monitor retains this resource across init reboots. Dropping it unregisters
/// only its own rule; existing host rules and the registry status are untouched.
pub struct Binfmt {
    directory: Option<TempDir>,
    entry: PathBuf,
    registered: bool,
}

impl Binfmt {
    /// Registers a statically linked host interpreter for the configured ELF ABI.
    ///
    /// Call only after making the monitor's mount namespace private. The kernel
    /// shares rules with other processes in the same user namespace. The F flag
    /// pins the interpreter, so it remains usable after `pivot_root` and in exec.
    ///
    /// # Errors
    /// Returns an error for an invalid interpreter, unavailable `CONFIG_BINFMT_MISC`,
    /// a disabled registry, or a failed registration. Partial setup is cleaned up.
    pub fn register(config: &BinfmtConfig, runtime_dir: &Path) -> Result<Self> {
        config.validate()?;
        validate_interpreter(&config.interpreter)?;
        let name = format!("kurumi-{}", Uuid::new_v4().simple());
        let registration = registration(&name, config)?;
        let directory = tempfile::Builder::new()
            .prefix(".binfmt-")
            .tempdir_in(runtime_dir)
            .context("failed to create private binfmt mountpoint")?;
        mount(
            Some(Path::new("binfmt_misc")),
            directory.path(),
            Some("binfmt_misc"),
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            None,
        )
        .context("failed to mount binfmt_misc; the host kernel must enable CONFIG_BINFMT_MISC")?;
        let status = directory.path().join("status");
        let register = directory.path().join("register");
        let mut resource = Self {
            entry: directory.path().join(name),
            directory: Some(directory),
            registered: false,
        };
        ensure!(
            fs::read_to_string(status)?.trim() == "enabled",
            "binfmt_misc is disabled on the host; enable it before starting this container"
        );
        OpenOptions::new()
            .write(true)
            .open(register)?
            .write_all(registration.as_bytes())
            .context("failed to register binfmt interpreter; the kernel must support the F flag")?;
        resource.registered = true;
        Ok(resource)
    }
}

impl Drop for Binfmt {
    fn drop(&mut self) {
        if self.registered
            && let Err(error) = fs::write(&self.entry, b"-1")
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, entry = %self.entry.display(), "failed to unregister binfmt rule");
        }
        if let Some(directory) = self.directory.take()
            && let Err(error) = unmount(directory.path(), true)
        {
            // Never let TempDir recursively remove files in a live shared registry.
            let path = directory.keep();
            tracing::warn!(%error, path = %path.display(), "failed to detach binfmt mount");
        }
    }
}

fn registration(name: &str, config: &BinfmtConfig) -> Result<String> {
    let (magic, mask) = elf::signature(config.arch);
    let mut value = format!(":{name}:M:0:");
    for bytes in [magic, mask] {
        for byte in bytes {
            write!(value, "\\x{byte:02x}").expect("writing into a String cannot fail");
        }
        value.push(':');
    }
    value.push_str(
        config
            .interpreter
            .to_str()
            .context("binfmt path must be UTF-8")?,
    );
    // F opens the interpreter at registration; C preserves the executable's
    // credentials (and implies O), as in ruri. QEMU receives the normal argv ABI.
    value.push_str(":OCF");
    ensure!(
        value.len() <= 1920,
        "binfmt registration exceeds the kernel's 1920-byte limit"
    );
    Ok(value)
}

/// Rejects foreign/dynamic interpreters before an F rule can affect the host.
fn validate_interpreter(path: &Path) -> Result<()> {
    let mut file = File::open(path).context("failed to open binfmt interpreter")?;
    let mut header = [0_u8; 64];
    file.read_exact(&mut header)
        .context("binfmt interpreter must be a static host ELF executable")?;
    let arch: BinfmtArch = serde_json::from_value(std::env::consts::ARCH.into())
        .context("unsupported host architecture for binfmt interpreter validation")?;
    let (magic, mask) = elf::signature(arch);
    ensure!(
        matches!(header[4], 1 | 2)
            && matches!(header[5], 1 | 2)
            && header
                .iter()
                .zip(magic)
                .zip(mask)
                .all(|((&actual, &expected), &mask)| actual & mask == expected & mask),
        "binfmt interpreter must be a host-native ELF executable"
    );
    let read_u16 = |offset| {
        let bytes = [header[offset], header[offset + 1]];
        if header[5] == 1 {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    };
    let (offset, size, count) = if header[4] == 2 {
        let bytes = header[32..40]
            .try_into()
            .expect("ELF64 offset has eight bytes");
        let offset = if header[5] == 1 {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        };
        (offset, read_u16(54), read_u16(56))
    } else {
        let bytes = header[28..32]
            .try_into()
            .expect("ELF32 offset has four bytes");
        let offset = if header[5] == 1 {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        };
        (u64::from(offset), read_u16(42), read_u16(44))
    };
    let file_size = file.metadata()?.len();
    ensure!(
        count > 0
            && count < 0xffff
            && size >= if header[4] == 2 { 56 } else { 32 }
            && offset
                .checked_add(u64::from(size) * u64::from(count))
                .is_some_and(|end| end <= file_size),
        "binfmt interpreter has an invalid ELF program header table"
    );
    for index in 0..count {
        file.seek(SeekFrom::Start(offset + u64::from(index) * u64::from(size)))?;
        let mut kind = [0_u8; 4];
        file.read_exact(&mut kind)?;
        let kind = if header[5] == 1 {
            u32::from_le_bytes(kind)
        } else {
            u32::from_be_bytes(kind)
        };
        ensure!(
            kind != 3,
            "binfmt interpreter must be statically linked (ELF PT_INTERP found)"
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/binfmt/tests.rs"]
mod tests;
