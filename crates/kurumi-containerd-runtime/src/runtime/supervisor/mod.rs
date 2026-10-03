use std::{fs::File, io, os::fd::AsFd};

use crate::{Result, error::ErrorContext as _};
use kurumi_containerd_helper::{
    process::{WaitStatus, dup_stdio, is_interrupted, read, waitpid},
    signal::{Signal, SignalActionFlags, SignalHandler, set_signal_handler},
};

pub(super) fn is_reboot_status(status: WaitStatus) -> bool {
    matches!(status, WaitStatus::Signaled(_, signal, _) if signal == Signal::Hangup.into())
}

pub(super) fn waitpid_retry(pid: i32) -> io::Result<WaitStatus> {
    loop {
        match waitpid(pid, false) {
            Err(error) if is_interrupted(&error) => {}
            result => return result,
        }
    }
}

pub(super) fn read_retry<Fd: AsFd>(fd: Fd, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        match read(&fd, buffer) {
            Err(error) if is_interrupted(&error) => {}
            result => return result,
        }
    }
}

pub(super) fn wait_status_code(status: WaitStatus) -> i32 {
    match status {
        WaitStatus::Exited(_, code) => code,
        WaitStatus::Signaled(_, signal, _) => 128 + signal.raw(),
        _ => 125,
    }
}

pub(super) fn redirect_stdio_to_null() {
    if let Ok(null) = File::options().read(true).write(true).open("/dev/null") {
        let _ = dup_stdio(&null);
    }
}

pub(super) fn configure_monitor_signals() -> Result<()> {
    for signal in [
        Signal::Terminate,
        Signal::Interrupt,
        Signal::Quit,
        Signal::Hangup,
        Signal::Pipe,
        Signal::User1,
        Signal::User2,
    ] {
        set_signal_handler(signal, SignalHandler::Ignore, SignalActionFlags::empty())
            .with_context(|| format!("failed to ignore monitor signal {signal}"))?;
    }
    Ok(())
}

pub(super) fn ignore_foreground_parent_signals() -> Result<()> {
    for signal in [Signal::Interrupt, Signal::Terminate] {
        set_signal_handler(signal, SignalHandler::Ignore, SignalActionFlags::empty())
            .with_context(|| format!("failed to ignore foreground parent signal {signal}"))?;
    }
    Ok(())
}

pub(super) fn reset_init_signals() {
    for signal in [
        Signal::Terminate,
        Signal::Interrupt,
        Signal::Quit,
        Signal::Hangup,
        Signal::Pipe,
        Signal::User1,
        Signal::User2,
    ] {
        let _ = set_signal_handler(signal, SignalHandler::Default, SignalActionFlags::empty());
    }
}

#[cfg(test)]
mod tests;
