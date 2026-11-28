// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::fs::File;
use std::os::fd::AsFd;

use super::KernelSu;

#[test]
fn skips_redirected_stdio_without_a_kernelsu_driver() {
    let file = File::open("/dev/null").unwrap();
    let driver = KernelSu {
        driver: file.try_clone().unwrap().into(),
    };
    assert!(!driver.wrap_tty(file.as_fd()).unwrap());
}

#[test]
fn failed_wrapper_ioctl_preserves_the_original_descriptor() {
    let file = File::open("/dev/null").unwrap();
    let driver = KernelSu {
        driver: file.try_clone().unwrap().into(),
    };
    assert!(driver.get_wrapped_fd(file.as_fd()).is_err());
    assert!(file.metadata().is_ok());
}

#[test]
fn failed_tty_wrapping_leaves_the_terminal_usable() {
    let driver = KernelSu {
        driver: File::open("/dev/null").unwrap().into(),
    };
    let pty = crate::terminal::open_pty(None).unwrap();
    assert!(driver.wrap_tty(pty.slave.as_fd()).is_err());
    assert!(crate::terminal::is_terminal(pty.slave.as_fd()).unwrap());
    assert!(crate::terminal::terminal_settings(pty.slave.as_fd()).is_ok());
}
