// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::os::fd::{AsFd, AsRawFd};

use kurumi_containerd_helper::kernelsu::KernelSu;

pub fn wrap_stdio() {
    let driver = match KernelSu::open() {
        Ok(driver) => driver,
        Err(error) => {
            tracing::debug!(%error, "KernelSU TTY wrapping is unavailable");
            return;
        }
    };
    for fd in [
        std::io::stdin().as_fd(),
        std::io::stdout().as_fd(),
        std::io::stderr().as_fd(),
    ] {
        if let Err(error) = driver.wrap_tty(fd) {
            tracing::warn!(fd = fd.as_raw_fd(), %error, "failed to wrap KernelSU TTY");
        }
    }
}
