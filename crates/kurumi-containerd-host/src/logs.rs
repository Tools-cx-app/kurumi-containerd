use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

use kurumi_containerd_helper::fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW, OPEN_NONBLOCK};
use kurumi_containerd_helper::process::effective_uid;

use crate::Result;
use crate::error::{ErrorContext as _, ensure};

#[cfg(target_os = "android")]
pub const SOCKET_PATH: &str = "/dev/kurumi/socket.sock";
#[cfg(target_os = "linux")]
pub const SOCKET_PATH: &str = "/var/kurumi/socket.sock";

/// Opens the private host directory shared by the daemon socket and logs.
/// # Errors
/// Returns an error if root cannot create or verify the directory.
pub fn socket_directory() -> Result<File> {
    ensure!(
        effective_uid() == 0,
        "the daemon socket directory requires root privileges"
    );
    let directory = Path::new(SOCKET_PATH)
        .parent()
        .context("socket path has no parent directory")?;
    match DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).with_context(|| format!("cannot create {}", directory.display()));
        }
    }
    let file = File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | OPEN_NOFOLLOW | OPEN_CLOEXEC)
        .open(directory)
        .with_context(|| format!("cannot open {}", directory.display()))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.uid() == 0 && metadata.mode().trailing_zeros() >= 6,
        "daemon socket directory must be private and owned by root: {}",
        directory.display()
    );
    Ok(file)
}

#[derive(Clone, Copy)]
pub enum LogFile {
    Client,
    Container,
    Daemon,
}

impl LogFile {
    /// Opens a private, append-only writer beside the daemon socket.
    /// # Errors
    /// Returns an error for an unsafe directory, unsafe file or I/O failure.
    pub fn open(self) -> Result<File> {
        let _directory = socket_directory()?;
        let path = Path::new(SOCKET_PATH).with_file_name(match self {
            Self::Client => "client.log",
            Self::Container => "container.log",
            Self::Daemon => "daemon.log",
        });
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(OPEN_CLOEXEC | OPEN_NOFOLLOW | OPEN_NONBLOCK | libc::O_NOCTTY)
            .open(&path)
            .with_context(|| format!("cannot open {}", path.display()))?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file()
                && metadata.uid() == 0
                && metadata.mode().trailing_zeros() >= 6
                && metadata.nlink() == 1,
            "log must be a private root-owned regular file with one link: {}",
            path.display()
        );
        Ok(file)
    }
}

pub(crate) struct ConsoleLog {
    file: File,
    record: Vec<u8>,
    prefix_len: usize,
}

impl ConsoleLog {
    pub(crate) fn new(file: File, name: &str) -> Self {
        let record = format!("[{name}] ").into_bytes();
        let prefix_len = record.len();
        Self {
            file,
            record,
            prefix_len,
        }
    }

    pub(crate) fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        const MAX_LINE: usize = 16 * 1024;
        for mut line in bytes.split_inclusive(|byte| *byte == b'\n') {
            while !line.is_empty() {
                let remaining = MAX_LINE - (self.record.len() - self.prefix_len);
                let (part, rest) = line.split_at(remaining.min(line.len()));
                self.record.extend_from_slice(part);
                if part.len() == remaining || part.ends_with(b"\n") {
                    self.flush()?;
                }
                line = rest;
            }
        }
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        if self.record.len() > self.prefix_len {
            if !self.record.ends_with(b"\n") {
                self.record.push(b'\n');
            }
            self.file.write_all(&self.record)?;
            self.record.truncate(self.prefix_len);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/logs/tests.rs"]
mod tests;
