use super::*;

use std::process::{Command, Stdio};

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
