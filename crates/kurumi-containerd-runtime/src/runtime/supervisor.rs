// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

#[cfg(feature = "servicectl")]
use super::state::store::StateStore;
#[cfg(feature = "servicectl")]
use crate::host::{
    cgroup::{Cgroup, terminate_cgroup},
    process::ProcessHandle,
};
use crate::{Result, error::ErrorContext as _};
#[cfg(feature = "servicectl")]
use kurumi_containerd_helper::terminal::{PollEvents, poll};
use kurumi_containerd_helper::{
    process::{WaitStatus, dup_stdio, is_interrupted, read, waitpid},
    signal::{Signal, SignalActionFlags, SignalHandler, set_signal_handler},
};
#[cfg(feature = "servicectl")]
use std::time::{Duration, Instant};
use std::{fs::File, io, os::fd::AsFd};

#[cfg(feature = "servicectl")]
pub(super) fn supervise_shared(
    child: i32,
    monitor: &impl AsFd,
    cgroup: &mut Cgroup,
    state_store: &StateStore,
) -> Result<WaitStatus> {
    let worker_pid = kurumi_containerd_helper::process::current_pid();
    let worker_start_time = crate::host::process::process_start_time(worker_pid)?;
    let directory = cgroup.directory()?;
    let init = ProcessHandle::open(child)?;
    let mut main_status = None;
    let mut monitor_gone = false;
    while main_status.is_none() {
        reap_children(child, &mut main_status)?;
        if main_status.is_some() {
            break;
        }
        match poll([(monitor.as_fd(), PollEvents::READABLE)], 50) {
            Ok([events]) if events.intersects(PollEvents::HANGUP | PollEvents::READABLE) => {
                monitor_gone = true;
                let _ = init.send_signal(Signal::Kill);
                break;
            }
            Ok(_) => {}
            Err(error) if is_interrupted(&error) => {}
            Err(error) => return Err(error).context("failed to watch monitor lifetime"),
        }
    }
    // Cgroup membership covers exec workers as well as double-forked descendants.
    terminate_cgroup(&directory, Duration::from_secs(5))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if reap_children(child, &mut main_status)? {
            break;
        }
        if Instant::now() >= deadline {
            crate::error::bail!("timed out reaping shared container descendants");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if monitor_gone {
        cgroup.remove()?;
        state_store.remove_shared_state_for_worker(worker_pid, worker_start_time)?;
    }
    main_status.context("shared supervisor exited without a wait status")
}

#[cfg(feature = "servicectl")]
fn reap_children(main_pid: i32, main_status: &mut Option<WaitStatus>) -> Result<bool> {
    loop {
        match waitpid(-1, true) {
            Ok(WaitStatus::StillAlive) => return Ok(false),
            Ok(status) => {
                let (WaitStatus::Exited(pid, _) | WaitStatus::Signaled(pid, _, _)) = status else {
                    continue;
                };
                if pid == main_pid {
                    *main_status = Some(status);
                }
            }
            Err(error) if error.raw_os_error() == Some(libc::ECHILD) => return Ok(true),
            Err(error) if is_interrupted(&error) => {}
            Err(error) => return Err(error).context("failed to reap shared container child"),
        }
    }
}

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
#[path = "../../tests/unit/runtime/supervisor/tests.rs"]
mod tests;
