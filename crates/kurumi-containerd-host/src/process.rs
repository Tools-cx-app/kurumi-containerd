use std::{
    os::fd::{AsFd, AsRawFd, OwnedFd},
    time::{Duration, Instant},
};

use crate::{
    Result,
    error::{ErrorContext as _, bail},
};
use kurumi_containerd_helper::{
    process::{is_interrupted, pidfd_open},
    signal::{SignalNumber, pidfd_send_signal},
};
use mio::{Events, Interest, Poll, Token, unix::SourceFd};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessId(i32);

impl ProcessId {
    #[must_use]
    pub const fn as_raw(self) -> i32 {
        self.0
    }
}

pub struct ProcessHandle {
    pid: ProcessId,
    fd: OwnedFd,
}

impl ProcessHandle {
    /// Opens a stable pidfd handle.
    ///
    /// # Errors
    /// Returns errors when the kernel cannot open the process handle.
    pub fn open(pid: i32) -> Result<Self> {
        let fd = pidfd_open(pid).with_context(|| format!("failed to open pidfd for PID {pid}"))?;
        Ok(Self {
            pid: ProcessId(pid),
            fd,
        })
    }

    #[must_use]
    pub const fn pid(&self) -> ProcessId {
        self.pid
    }

    /// Sends a signal through the pidfd.
    ///
    /// # Errors
    /// Returns errors when signaling the process fails.
    pub fn send_signal(&self, signal: impl Into<SignalNumber>) -> Result<()> {
        pidfd_send_signal(self.fd.as_fd(), signal.into())
            .with_context(|| format!("failed to signal PID {} through pidfd", self.pid.as_raw()))
    }

    /// Waits up to the timeout for process exit.
    ///
    /// # Errors
    /// Returns errors when registering or polling the pidfd fails.
    pub fn wait_for_exit(&self, timeout: Duration) -> Result<bool> {
        let mut poll = Poll::new().context("failed to create pidfd poller")?;
        poll.registry()
            .register(
                &mut SourceFd(&self.fd.as_raw_fd()),
                Token(0),
                Interest::READABLE,
            )
            .context("failed to register pidfd")?;
        let mut events = Events::with_capacity(1);
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match poll.poll(&mut events, Some(remaining.min(Duration::from_millis(100)))) {
                Ok(()) => {}
                Err(error) if is_interrupted(&error) => continue,
                Err(error) => return Err(error).context("failed to poll pidfd"),
            }
            if !events.is_empty() {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
        }
    }
}

/// Opens a handle after validating that the PID is positive.
///
/// # Errors
/// Returns errors for invalid PIDs or failed pidfd access.
pub fn require_handle(pid: i32) -> Result<ProcessHandle> {
    if pid <= 0 {
        bail!("invalid process PID {pid}");
    }
    ProcessHandle::open(pid)
}

/// Reads a process's parent PID.
///
/// # Errors
/// Returns errors when procfs cannot be read or parsed.
pub fn parent_pid(pid: i32) -> Result<i32> {
    Ok(procfs::process::Process::new(pid)?.stat()?.ppid)
}

/// Reads the current host boot identity.
///
/// # Errors
/// Returns errors when the kernel boot ID cannot be read.
pub fn host_boot_id() -> Result<String> {
    Ok(procfs::sys::kernel::random::boot_id()?)
}

/// Reads a process's start time in kernel clock ticks.
///
/// # Errors
/// Returns errors when procfs cannot be read or parsed.
pub fn process_start_time(pid: i32) -> Result<u64> {
    Ok(procfs::process::Process::new(pid)?.stat()?.starttime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_probes_current_process() {
        let pid = kurumi_containerd_helper::process::current_pid();
        let process = ProcessHandle::open(pid).unwrap();
        process.send_signal(SignalNumber::NONE).unwrap();
        assert_eq!(process.pid().as_raw(), pid);
        assert_eq!(
            host_boot_id().unwrap(),
            procfs::sys::kernel::random::boot_id().unwrap()
        );
        assert_eq!(
            process_start_time(pid).unwrap(),
            procfs::process::Process::new(pid)
                .unwrap()
                .stat()
                .unwrap()
                .starttime
        );
        assert!(!process.wait_for_exit(Duration::ZERO).unwrap());
    }
}
