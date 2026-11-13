// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use anyhow::Result;
use kurumi_containerd_config::ConfigEntry;
use kurumi_containerd_runtime::ContainerInfo;

pub(super) struct Entry {
    pub(super) config: ConfigEntry,
    pub(super) info: Result<ContainerInfo>,
    pub(super) foreground: bool,
    pub(super) install_ready: bool,
}
