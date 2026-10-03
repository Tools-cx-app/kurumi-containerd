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
