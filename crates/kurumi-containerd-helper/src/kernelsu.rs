// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

//! Android `KernelSU` file descriptor wrapping.

use std::fs;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;

use crate::syscall::cvt;
use crate::terminal::is_terminal;

// ReSukiSU uapi/supercall.h: _IOC(_IOC_WRITE, 'K', 15, 0). The encoded
// size is deliberately zero, although the argument contains two __u32 fields.
const GET_WRAPPER_FD: libc::Ioctl = 0x4000_4b0f;
const INSTALL_MAGIC1: libc::c_uint = 0xdead_beef;
const INSTALL_MAGIC2: libc::c_uint = 0xcafe_babe;

#[repr(C)]
struct GetWrapperFd {
    fd: u32,
    flags: u32,
}

pub struct KernelSu {
    driver: OwnedFd,
}

impl KernelSu {
    /// Acquires a private driver descriptor, preferring an inherited su-session
    /// driver over a general driver before requesting a new one from `KernelSU`.
    pub fn open() -> io::Result<Self> {
        if let Some(driver) = inherited_driver()? {
            return Ok(Self { driver });
        }
        let mut fd: libc::c_int = -1;
        // SAFETY: the magic values and zero command select KernelSU's install
        // ABI, whose output is a writable c_int. Without KernelSU these magic
        // values do not match Linux's reboot magic, so no reboot is requested.
        let result = unsafe {
            libc::syscall(
                libc::SYS_reboot,
                libc::c_ulong::from(INSTALL_MAGIC1),
                libc::c_ulong::from(INSTALL_MAGIC2),
                0 as libc::c_ulong,
                &raw mut fd,
            )
        };
        // KernelSU may install the descriptor while the original syscall still
        // returns an error. Its out parameter determines whether it succeeded.
        if fd < 0 {
            return Err(if result == -1 {
                io::Error::last_os_error()
            } else {
                io::Error::new(io::ErrorKind::NotFound, "KernelSU driver is unavailable")
            });
        }
        // SAFETY: KernelSU installed a newly owned descriptor in fd.
        let driver = unsafe { OwnedFd::from_raw_fd(fd) };
        set_cloexec(driver.as_fd())?;
        Ok(Self { driver })
    }

    /// Returns a newly owned wrapper; the original descriptor stays open.
    pub fn get_wrapped_fd(&self, fd: BorrowedFd<'_>) -> io::Result<OwnedFd> {
        let command = GetWrapperFd {
            fd: u32::try_from(fd.as_raw_fd())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "negative descriptor"))?,
            flags: 0,
        };
        // SAFETY: both descriptors remain live and command has the exact C ABI
        // layout expected by GET_WRAPPER_FD (two initialized __u32 fields).
        let wrapped = cvt(unsafe {
            libc::ioctl(self.driver.as_raw_fd(), GET_WRAPPER_FD, &raw const command)
        })?;
        // SAFETY: a successful GET_WRAPPER_FD returns a newly owned descriptor.
        let wrapped = unsafe { OwnedFd::from_raw_fd(wrapped) };
        set_cloexec(wrapped.as_fd())?;
        Ok(wrapped)
    }

    /// Replaces a TTY descriptor with its wrapper, returning whether it changed.
    /// An EACCES from the original TTY check is retried through the wrapper, as
    /// the active root profile may prevent querying the original terminal.
    pub fn wrap_tty(&self, fd: BorrowedFd<'_>) -> io::Result<bool> {
        match is_terminal(fd) {
            Ok(true) => {}
            Err(error) if error.raw_os_error() == Some(libc::EACCES) => {}
            _ => return Ok(false),
        }
        let wrapped = self.get_wrapped_fd(fd)?;
        if !is_terminal(wrapped.as_fd()).unwrap_or(false) {
            return Ok(false);
        }
        // SAFETY: wrapped and fd are distinct live descriptors; dup2 replaces
        // fd without leaving it closed and does not transfer wrapped's ownership.
        cvt(unsafe { libc::dup2(wrapped.as_raw_fd(), fd.as_raw_fd()) })?;
        Ok(true)
    }
}

fn inherited_driver() -> io::Result<Option<OwnedFd>> {
    let mut driver = None;
    // Finish enumeration before duplicating descriptors, so our duplicates
    // cannot become new candidates during the same directory traversal.
    let entries = fs::read_dir("/proc/self/fd")?.collect::<io::Result<Vec<_>>>()?;
    for entry in entries {
        let Ok(target) = fs::read_link(entry.path()) else {
            continue;
        };
        let su_session = target == Path::new("anon_inode:[ksu_driver_su]");
        if !su_session && target != Path::new("anon_inode:[ksu_driver]") {
            continue;
        }
        let Some(fd) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<RawFd>().ok())
            .filter(|fd| *fd >= 0)
        else {
            continue;
        };
        // SAFETY: fcntl validates the raw descriptor and returns a newly owned
        // duplicate. No BorrowedFd or owner is made from the scanned raw number.
        let duplicate = cvt(unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) })?;
        // SAFETY: F_DUPFD_CLOEXEC returned a newly owned descriptor.
        let duplicate = unsafe { OwnedFd::from_raw_fd(duplicate) };
        // Validate the owned duplicate too: another thread could have replaced
        // the scanned descriptor between read_link and fcntl.
        if fs::read_link(format!("/proc/self/fd/{}", duplicate.as_raw_fd()))? != target {
            continue;
        }
        // Restore CLOEXEC on the inherited original too, so a su-session
        // descriptor cannot leak into a container's eventual exec.
        set_cloexec_raw(fd)?;
        if su_session {
            return Ok(Some(duplicate));
        }
        driver = Some(duplicate);
    }
    Ok(driver)
}

fn set_cloexec(fd: BorrowedFd<'_>) -> io::Result<()> {
    set_cloexec_raw(fd.as_raw_fd())
}

fn set_cloexec_raw(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl validates fd and these commands use integer flags only.
    let flags = cvt(unsafe { libc::fcntl(fd, libc::F_GETFD) })?;
    // SAFETY: flags preserves existing descriptor flags and adds FD_CLOEXEC.
    cvt(unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) }).map(drop)
}

#[cfg(test)]
#[path = "../tests/unit/kernelsu/tests.rs"]
mod tests;
