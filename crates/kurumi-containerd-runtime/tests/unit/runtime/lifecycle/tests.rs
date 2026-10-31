// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

use std::{
    io::BufRead,
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    thread,
};

#[test]
fn startup_errors_reach_parent_or_require_fallback_logging() {
    let (reader, mut writer) = pipe().unwrap();
    assert!(report_startup_error(
        Some(&mut writer),
        "boot failed: mount denied"
    ));
    drop(writer);
    let mut received = String::new();
    File::from(reader).read_to_string(&mut received).unwrap();
    assert_eq!(received, "boot failed: mount denied");
    assert!(!report_startup_error(None, "reboot failed"));
    let mut read_only: OwnedFd = File::open("/dev/null").unwrap().into();
    assert!(!report_startup_error(Some(&mut read_only), "boot failed"));
}

#[test]
fn startup_failure_terminates_init_and_reaps_worker() {
    let mut worker = Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let mut init = Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let worker_pid = i32::try_from(worker.id()).unwrap();
    let init_pid = i32::try_from(init.id()).unwrap();
    let init_handle = ProcessHandle::open(init_pid).unwrap();
    let result = (|| -> Result<()> {
        let _processes = GenerationProcesses {
            worker: Some(worker_pid),
            init: Some(ProcessHandle::open(init_pid)?),
            network_writer: None,
        };
        Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "startup reader closed").into())
    })();
    let init_exited = init_handle.wait_for_exit(Duration::from_secs(2)).unwrap();
    let worker_reaped =
        waitpid(worker_pid, true).is_err_and(|error| error.raw_os_error() == Some(libc::ECHILD));
    let _ = init.kill();
    let _ = init.wait();
    let _ = worker.kill();
    let _ = worker.wait();
    assert!(result.is_err());
    assert!(init_exited, "startup failure left init running");
    assert!(worker_reaped, "startup failure left worker unreaped");
}

#[test]
fn pid_handshake_failure_reaps_worker_without_init_handle() {
    let (reader, writer) = pipe().unwrap();
    let mut worker = Command::new("sh")
        .args(["-c", "cat >/dev/null"])
        .stdin(Stdio::from(reader))
        .spawn()
        .unwrap();
    let pid = i32::try_from(worker.id()).unwrap();
    drop(GenerationProcesses {
        worker: Some(pid),
        init: None,
        network_writer: Some(writer),
    });
    let reaped = waitpid(pid, true).is_err_and(|error| error.raw_os_error() == Some(libc::ECHILD));
    let _ = worker.kill();
    let _ = worker.wait();
    assert!(reaped, "PID handshake failure left worker unreaped");
}

#[test]
fn reboot_waits_for_ordinary_operations_but_yields_to_explicit_stop() {
    for stop in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::new(directory.path(), "demo");
        std::fs::create_dir(store.state_dir()).unwrap();
        let lifecycle = store.lock().unwrap();
        let stop_request = stop.then(|| store.request_stop().unwrap());
        let workdir = directory.path().to_path_buf();
        let (started_sender, started_receiver) = mpsc::channel();
        let (finished_sender, finished_receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let store = StateStore::new(&workdir, "demo");
            started_sender.send(()).unwrap();
            let lock = wait_for_reboot_lock(&store).unwrap();
            finished_sender.send(lock.is_some()).unwrap();
        });
        started_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let result = finished_receiver.recv_timeout(if stop {
            Duration::from_secs(2)
        } else {
            Duration::from_millis(50)
        });
        drop(stop_request);
        drop(lifecycle);
        worker.join().unwrap();
        if stop {
            assert_eq!(result, Ok(false), "reboot ignored an explicit stop");
        } else {
            assert_eq!(
                result,
                Err(RecvTimeoutError::Timeout),
                "lock contention cancelled reboot"
            );
            assert!(
                finished_receiver
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
            );
        }
    }
}

#[test]
fn custom_init_can_clean_up_before_exit_and_is_killed_only_after_timeout() {
    for graceful in [false, true] {
        let mut child = Command::new("sh")
            .args(["-c", if graceful {
                "trap 'printf cleaned; exit 0' TERM; printf 'ready\\n'; while :; do read -r line; done"
            } else {
                "trap '' TERM; printf 'ready\\n'; while :; do read -r line; done"
            }])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn().unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut ready = String::new();
        output.read_line(&mut ready).unwrap();
        assert_eq!(ready, "ready\n");
        let process = ProcessHandle::open(i32::try_from(child.id()).unwrap()).unwrap();
        let timeout = if graceful {
            Duration::from_secs(2)
        } else {
            Duration::from_millis(50)
        };
        let result = shutdown_init(&process, init::InitSystem::Custom, timeout);
        let _ = child.kill();
        let status = child.wait().unwrap();
        result.unwrap();
        let mut remaining = String::new();
        output.read_to_string(&mut remaining).unwrap();
        if graceful {
            assert!(status.success());
            assert_eq!(remaining, "cleaned");
        } else {
            assert_eq!(status.signal(), Some(Signal::Kill.raw()));
        }
    }
}
