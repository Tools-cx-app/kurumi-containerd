// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

//! Host resources used to construct and supervise a container.

use kurumi_containerd_error as error;
pub use kurumi_containerd_error::{Result, RuntimeError};

#[cfg(target_os = "android")]
pub mod android;
mod archive;
pub mod cgroup;
pub mod check;
pub mod network;
pub mod process;
pub mod rootfs;
#[cfg(any(target_os = "android", test))]
mod selinux;
pub mod terminal;
