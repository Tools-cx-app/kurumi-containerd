// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::fs::File;
use std::io::{self, Seek, Write};
use std::os::fd::AsRawFd;

use fs2::FileExt;
use kurumi_containerd_helper::fs::anonymous_memory_file;
use serde::{Serialize, de::DeserializeOwned};

/// An anonymous, fork-shared file used to coordinate host and runtime processes.
/// Reopening it creates an independent file description for advisory locking.
pub struct MemoryFile(File);

impl MemoryFile {
    /// Allocates an anonymous file with no filesystem directory entry.
    ///
    /// # Errors
    /// Returns an error if the kernel cannot create the descriptor.
    pub fn new(name: &str) -> io::Result<Self> {
        Ok(Self(anonymous_memory_file(name)?))
    }

    /// Opens and exclusively locks the file until the returned descriptor closes.
    ///
    /// # Errors
    /// Returns an error if the descriptor cannot be reopened or locked.
    pub fn lock(&self) -> io::Result<File> {
        let file = self.open()?;
        FileExt::lock_exclusive(&file)?;
        Ok(file)
    }

    /// Returns immediately if another process holds the lock.
    ///
    /// # Errors
    /// Returns an error if opening or locking fails.
    pub fn try_lock(&self) -> io::Result<Option<File>> {
        let file = self.open()?;
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(file)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Reads a value and commits its replacement while holding the shared lock.
    ///
    /// # Errors
    /// Returns errors from reading, the operation, or publishing the replacement.
    pub fn update<T, R>(
        &self,
        operation: impl FnOnce(&mut T) -> crate::Result<R>,
    ) -> crate::Result<R>
    where
        T: Default + DeserializeOwned + Serialize,
    {
        let mut file = self.lock()?;
        let mut value = if file.metadata()?.len() == 0 {
            T::default()
        } else {
            serde_json::from_reader(&mut file)?
        };
        let result = operation(&mut value)?;
        let data = serde_json::to_vec(&value)?;
        file.rewind()?;
        file.write_all(&data)?;
        file.set_len(data.len() as u64)?;
        Ok(result)
    }

    fn open(&self) -> io::Result<File> {
        File::options()
            .read(true)
            .write(true)
            .open(format!("/proc/self/fd/{}", self.0.as_raw_fd()))
    }
}
