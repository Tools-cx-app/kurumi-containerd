use std::{
    io::Write,
    os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
    sync::atomic::{AtomicI32, Ordering},
};

use super::process::ProcessHandle;
use crate::{
    Result,
    error::{ErrorContext as _, bail},
};
use kurumi_containerd_helper::{
    fs::set_file_mode,
    process::{
        ForkResult, WaitStatus, dup_stdio, effective_uid, fork, is_interrupted, is_io_error,
        is_would_block, read, set_gid, set_groups, set_uid, setsid, waitpid, write,
    },
    signal::{Signal, SignalActionFlags, SignalHandler, SignalNumber, set_signal_handler},
    terminal::{
        PtyPair, TerminalSettings, WindowSize, is_terminal, make_raw, open_pty, pty_number,
        receive_fds, send_fds, set_controlling_terminal, set_nonblocking, set_terminal_settings,
        set_terminal_size, socket_pair, terminal_settings, terminal_size,
    },
};

static FORWARDED_SIGNAL: AtomicI32 = AtomicI32::new(0);
const PTY_MODE: u32 = 0o620;

pub(crate) struct Console {
    pub(crate) master: OwnedFd,
    pub(crate) slave_path: String,
}

impl Console {
    pub(crate) fn open() -> Result<Self> {
        Self::open_from(std::io::stdin().as_fd())
    }

    fn open_from(source: BorrowedFd<'_>) -> Result<Self> {
        let winsize = terminal_size(source).ok();
        let settings = terminal_settings(source).ok();
        let PtyPair { master, slave } = if effective_uid() == 0 {
            open_console_pty_unprivileged(winsize.as_ref())?
        } else {
            open_console_pty(winsize.as_ref())?
        };
        if let Some(settings) = &settings {
            set_terminal_settings(slave.as_fd(), settings)
                .context("failed to copy terminal settings to PTY")?;
        }
        // Keep the broker-owned UID on the host PTY, but pin the expected tty mode.
        let _ = set_file_mode(slave.as_fd(), PTY_MODE);
        Ok(Self {
            slave_path: tty_path(&master)?,
            master,
        })
    }

    pub(crate) fn open_slave(&self) -> Result<OwnedFd> {
        open_pty_slave(&self.slave_path)
    }
}

fn open_pty_slave(path: &str) -> Result<OwnedFd> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open PTY slave {path}"))
        .map(Into::into)
}

fn tty_path(master: &OwnedFd) -> Result<String> {
    let number = pty_number(master.as_fd()).context("failed to resolve PTY slave path")?;
    Ok(format!("/dev/pts/{number}"))
}

#[allow(unsafe_code)]
fn open_console_pty_unprivileged(winsize: Option<&WindowSize>) -> Result<PtyPair> {
    let (parent, child) = socket_pair().context("failed to create PTY broker channel")?;
    // SAFETY: this runtime is single-threaded at PTY allocation. The child only
    // drops credentials, allocates descriptors, sends them, and exits.
    match unsafe { fork() }.context("failed to fork PTY broker")? {
        ForkResult::Child => {
            drop(parent);
            let code = match drop_pty_broker_privileges()
                .and_then(|()| open_console_pty(winsize))
                .and_then(|pty| send_pty(&child, &pty))
            {
                Ok(()) => 0,
                Err(error) => {
                    tracing::error!("PTY broker failed: {error:#}");
                    1
                }
            };
            std::process::exit(code);
        }
        ForkResult::Parent { child: broker } => {
            drop(child);
            let received = receive_pty(&parent);
            drop(parent);
            let status = waitpid(broker, false).context("failed waiting for PTY broker")?;
            ensure_broker_success(status)?;
            received
        }
    }
}

fn drop_pty_broker_privileges() -> Result<()> {
    let id = pty_broker_id();
    set_groups(&[]).context("failed to clear PTY broker supplementary groups")?;
    set_gid(id).context("failed to drop PTY broker GID")?;
    set_uid(id).context("failed to drop PTY broker UID")?;
    Ok(())
}

#[cfg(target_os = "android")]
const fn pty_broker_id() -> u32 {
    9_999
}

#[cfg(not(target_os = "android"))]
const fn pty_broker_id() -> u32 {
    65_534
}

fn send_pty(socket: &OwnedFd, pty: &PtyPair) -> Result<()> {
    let descriptors = [pty.master.as_raw_fd(), pty.slave.as_raw_fd()];
    send_fds(socket.as_fd(), &descriptors).context("failed to send PTY descriptors")?;
    Ok(())
}

fn receive_pty(socket: &OwnedFd) -> Result<PtyPair> {
    let mut descriptors = receive_fds(socket.as_fd(), 2)
        .context("PTY broker exited without providing two descriptors")?
        .into_iter();
    Ok(PtyPair {
        master: descriptors.next().expect("helper returned two descriptors"),
        slave: descriptors.next().expect("helper returned two descriptors"),
    })
}

fn ensure_broker_success(status: WaitStatus) -> Result<()> {
    match status {
        WaitStatus::Exited(_, 0) => Ok(()),
        WaitStatus::Exited(_, code) => bail!("PTY broker exited with status {code}"),
        WaitStatus::Signaled(_, signal, _) => bail!("PTY broker terminated by {signal}"),
        status => bail!("unexpected PTY broker status: {status:?}"),
    }
}

fn open_console_pty(winsize: Option<&WindowSize>) -> Result<PtyPair> {
    open_pty(winsize).context("failed to allocate foreground console PTY")
}

pub(crate) fn configure_child(slave: &OwnedFd) -> Result<()> {
    setsid().context("failed to create terminal session")?;
    set_controlling_terminal(slave.as_fd()).context("failed to set controlling terminal")?;
    dup_stdio(slave).context("failed to connect console stdio")?;
    Ok(())
}

pub(crate) fn ignore_hangup() -> Result<()> {
    set_signal_handler(
        Signal::Hangup,
        SignalHandler::Ignore,
        SignalActionFlags::empty(),
    )
    .context("failed to ignore interactive SIGHUP")?;
    Ok(())
}

pub(crate) fn send_fd(socket: &OwnedFd, fd: &OwnedFd) -> Result<()> {
    let descriptors = [fd.as_raw_fd()];
    send_fds(socket.as_fd(), &descriptors).context("failed to send interactive PTY")?;
    Ok(())
}

pub(crate) fn receive_fd(socket: &OwnedFd) -> Result<OwnedFd> {
    receive_fds(socket.as_fd(), 1)
        .context("namespace worker exited before providing an interactive PTY")?
        .into_iter()
        .next()
        .context("helper returned no interactive PTY descriptor")
}

pub(crate) fn drain(master: &OwnedFd, child: i32) -> Result<WaitStatus> {
    set_nonblocking(master.as_fd()).context("failed to set PTY nonblocking")?;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        if poll_terminal(master.as_fd(), master.as_fd(), false, false)?.0 {
            match read(master, &mut buffer) {
                Ok(_) => {}
                Err(error)
                    if is_interrupted(&error) || is_would_block(&error) || is_io_error(&error) => {}
                Err(error) => return Err(error).context("failed to drain background PTY"),
            }
        }
        match waitpid(child, true) {
            Ok(WaitStatus::StillAlive) => {}
            Ok(status) => return Ok(status),
            Err(error) if is_interrupted(&error) => {}
            Err(error) => return Err(error).context("failed waiting for generation worker"),
        }
    }
}

pub(crate) fn proxy(
    master: &OwnedFd,
    child: i32,
    shutdown_target: Option<(&ProcessHandle, &dyn Fn() -> Result<()>)>,
) -> Result<WaitStatus> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let _raw_terminal = RawTerminal::enable(stdin.as_fd())?;
    let _signal_forwarding = shutdown_target
        .map(|_| ForwardSignals::install())
        .transpose()?;
    let mut stdin_open = true;
    let mut child_status = None;
    let mut quiet_polls_after_exit = 0_u8;
    let mut buffer = [0_u8; 16 * 1024];
    let mut pending_input = Vec::new();
    set_nonblocking(master.as_fd()).context("failed to set PTY nonblocking")?;
    loop {
        if let Some((target, _)) = shutdown_target {
            let signal = FORWARDED_SIGNAL.swap(0, Ordering::Relaxed);
            if let Some(signal) = SignalNumber::new(signal) {
                target
                    .send_signal(signal)
                    .context("failed to forward foreground signal")?;
            }
        }
        sync_terminal_size(stdin.as_fd(), master.as_fd())?;
        let (output_ready, read_input, write_ready) = poll_terminal(
            master.as_fd(),
            stdin.as_fd(),
            stdin_open,
            !pending_input.is_empty(),
        )?;

        if output_ready {
            match read(master, &mut buffer) {
                Ok(0) => {
                    if let Some(status) = child_status {
                        stdout.flush()?;
                        return Ok(status);
                    }
                }
                Err(error) if is_io_error(&error) => {
                    if let Some(status) = child_status {
                        stdout.flush()?;
                        return Ok(status);
                    }
                }
                Ok(length) => {
                    stdout.write_all(&buffer[..length])?;
                    stdout.flush()?;
                }
                Err(error) if is_interrupted(&error) || is_would_block(&error) => {}
                Err(error) => return Err(error).context("failed to read PTY output"),
            }
        }
        if read_input {
            match read(&stdin, &mut buffer) {
                Ok(0) => stdin_open = false,
                Ok(length) => {
                    pending_input.extend_from_slice(proxy_input(
                        &buffer[..length],
                        shutdown_target.map(|(_, shutdown)| shutdown),
                    )?);
                }
                Err(error) if is_interrupted(&error) || is_would_block(&error) => {}
                Err(error) => return Err(error).context("failed to read terminal input"),
            }
        }
        if write_ready && !pending_input.is_empty() {
            match write(master, &pending_input) {
                Ok(length) => {
                    pending_input.drain(..length);
                }
                Err(error) if is_interrupted(&error) || is_would_block(&error) => {}
                Err(error) => return Err(error).context("failed to write PTY input"),
            }
        }
        if child_status.is_none() {
            match waitpid(child, true)? {
                WaitStatus::StillAlive => {}
                status => child_status = Some(status),
            }
        }
        if let Some(status) = child_status {
            if output_ready {
                quiet_polls_after_exit = 0;
            } else {
                quiet_polls_after_exit += 1;
                if quiet_polls_after_exit >= 2 {
                    stdout.flush()?;
                    return Ok(status);
                }
            }
        }
    }
}

fn proxy_input<'a>(input: &'a [u8], shutdown: Option<&dyn Fn() -> Result<()>>) -> Result<&'a [u8]> {
    if let Some(shutdown) = shutdown
        && let Some(remaining) = input.strip_prefix(&[0x1b, 0x11])
    {
        shutdown().context("failed to request foreground shutdown")?;
        return Ok(remaining);
    }
    Ok(input)
}

#[allow(unsafe_code)]
fn poll_terminal(
    master: BorrowedFd<'_>,
    stdin: BorrowedFd<'_>,
    stdin_open: bool,
    write_pending: bool,
) -> Result<(bool, bool, bool)> {
    let mut descriptors = [
        libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: stdin.as_raw_fd(),
            events: if stdin_open { libc::POLLIN } else { 0 },
            revents: 0,
        },
    ];
    if write_pending {
        descriptors[0].events |= libc::POLLOUT;
    }
    loop {
        // SAFETY: descriptors points to two initialized pollfd values.
        let result = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, 100) };
        if result >= 0 {
            return Ok((
                descriptors[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0,
                descriptors[1].revents & libc::POLLIN != 0,
                descriptors[0].revents & libc::POLLOUT != 0,
            ));
        }
        let error = std::io::Error::last_os_error();
        if !is_interrupted(&error) {
            return Err(error).context("failed to poll terminal proxy");
        }
    }
}

struct RawTerminal {
    original: Option<TerminalSettings>,
}

impl RawTerminal {
    fn enable(fd: std::os::fd::BorrowedFd<'_>) -> Result<Self> {
        if !is_terminal(fd)? {
            return Ok(Self { original: None });
        }
        let original = terminal_settings(fd)?;
        let mut raw = original.clone();
        make_raw(&mut raw);
        set_terminal_settings(fd, &raw)?;
        Ok(Self {
            original: Some(original),
        })
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        if let Some(original) = &self.original {
            let _ = set_terminal_settings(std::io::stdin().as_fd(), original);
        }
    }
}

struct ForwardSignals;

impl ForwardSignals {
    fn install() -> Result<Self> {
        extern "C" fn record(signal: i32) {
            FORWARDED_SIGNAL.store(signal, Ordering::Relaxed);
        }

        for signal in [Signal::Interrupt, Signal::Terminate] {
            set_signal_handler(
                signal,
                SignalHandler::Handler(record),
                SignalActionFlags::RESTART,
            )
            .with_context(|| format!("failed to capture foreground signal {signal}"))?;
        }
        Ok(Self)
    }
}

impl Drop for ForwardSignals {
    fn drop(&mut self) {
        for signal in [Signal::Interrupt, Signal::Terminate] {
            let _ = set_signal_handler(signal, SignalHandler::Ignore, SignalActionFlags::empty());
        }
    }
}

fn sync_terminal_size(
    source: std::os::fd::BorrowedFd<'_>,
    target: std::os::fd::BorrowedFd<'_>,
) -> Result<()> {
    if is_terminal(source)? {
        let size = terminal_size(source)?;
        set_terminal_size(target, &size).context("failed to resize PTY")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_input_delegates_shutdown_and_preserves_input() {
        let calls = std::cell::Cell::new(0);
        let shutdown = || {
            calls.set(calls.get() + 1);
            Ok(())
        };
        assert_eq!(
            proxy_input(b"\x1b\x11rest", Some(&shutdown)).unwrap(),
            b"rest"
        );
        assert_eq!(
            proxy_input(b"ordinary", Some(&shutdown)).unwrap(),
            b"ordinary"
        );
        assert_eq!(proxy_input(b"\x1b", Some(&shutdown)).unwrap(), b"\x1b");
        assert_eq!(proxy_input(b"\x1b\x11", None).unwrap(), b"\x1b\x11");
        assert_eq!(calls.get(), 1);
        let failure = || Err(std::io::Error::other("shutdown failed").into());
        assert!(proxy_input(b"\x1b\x11", Some(&failure)).is_err());
    }

    #[cfg(target_os = "android")]
    #[test]
    fn uses_android_nobody_for_pty_broker() {
        assert_eq!(pty_broker_id(), 9_999);
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn uses_linux_nobody_for_pty_broker() {
        assert_eq!(pty_broker_id(), 65_534);
    }

    #[test]
    fn allocates_terminal_pair() {
        let console = Console::open().expect("PTY allocation should work");
        let slave = console.open_slave().expect("PTY slave should reopen");
        assert!(is_terminal(console.master.as_fd()).unwrap());
        assert!(is_terminal(slave.as_fd()).unwrap());
    }

    #[test]
    fn transfers_terminal_descriptor() {
        let (sender, receiver) = socket_pair().unwrap();
        let console = Console::open().unwrap();
        send_fd(&sender, &console.master).unwrap();
        let transferred = receive_fd(&receiver).unwrap();
        assert!(is_terminal(transferred.as_fd()).unwrap());
    }

    #[test]
    fn copies_source_terminal_settings() {
        let source = open_pty(None).unwrap();
        let mut settings = terminal_settings(source.slave.as_fd()).unwrap();
        make_raw(&mut settings);
        set_terminal_settings(source.slave.as_fd(), &settings).unwrap();

        let console = Console::open_from(source.slave.as_fd()).unwrap();
        let slave = console.open_slave().unwrap();
        write(&console.master, b"x").unwrap();

        assert!(
            poll_terminal(slave.as_fd(), source.master.as_fd(), false, false)
                .unwrap()
                .0
        );
    }
}
