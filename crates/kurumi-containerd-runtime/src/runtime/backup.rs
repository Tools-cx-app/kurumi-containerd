// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::path::Path;

use crate::{Result, Runtime, error::ensure};

impl Runtime {
    /// Exports a stopped container's rootfs to a new local archive.
    ///
    /// # Errors
    /// Returns an error for a running container, missing privileges, unsupported
    /// formats, existing outputs, or failed filesystem operations.
    pub fn backup(&self, output: &Path) -> Result<()> {
        Self::ensure_root()?;
        self.ensure_layout()?;
        let _lock = self.lock()?;
        self.scan()?;
        ensure!(
            self.state()?.is_none(),
            "container '{}' is running; stop it before backup",
            self.config.container.name
        );
        self.rootfs.backup(output)
    }
}
