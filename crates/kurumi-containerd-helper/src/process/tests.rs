use std::{
    os::fd::{AsFd, AsRawFd},
    process::Command,
};

use super::*;

#[test]
fn prevents_non_stdio_descriptor_inheritance() {
    let file = tempfile::tempfile().unwrap();
    // SAFETY: file owns a live descriptor and zero clears FD_CLOEXEC for this test.
    assert_ne!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) },
        -1
    );

    close_non_stdio_on_exec().unwrap();

    // SAFETY: file still owns the live descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags, -1);
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
}

#[test]
fn closes_non_stdio_descriptors_on_exec() {
    const CHILD: &str = "KURUMI_FD_EXEC_TEST";
    if std::env::var_os(CHILD).is_some() {
        let file = tempfile::tempfile().unwrap();
        // SAFETY: file owns a live descriptor and zero clears FD_CLOEXEC for this test.
        assert_ne!(
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) },
            -1
        );
        let shell = CString::new("/bin/sh").unwrap();
        let arguments = [
            CString::new("sh").unwrap(),
            CString::new("-c").unwrap(),
            CString::new(format!("test ! -e /proc/self/fd/{}", file.as_raw_fd())).unwrap(),
        ];
        execve(&shell, &arguments, &[]).unwrap();
        unreachable!();
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process::tests::closes_non_stdio_descriptors_on_exec",
        ])
        .env(CHILD, "1")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn closes_descriptors_outside_allowlist() {
    let kept = tempfile::tempfile().unwrap();
    assert!(!should_close_fd(libc::STDERR_FILENO, &[kept.as_fd()]));
    assert!(!should_close_fd(kept.as_raw_fd(), &[kept.as_fd()]));
    assert!(should_close_fd(kept.as_raw_fd() + 1, &[kept.as_fd()]));
}
