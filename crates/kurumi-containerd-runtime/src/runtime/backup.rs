// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::path::Path;

use crate::{Result, Runtime, error::ensure, host::package::ContainerPackage};

impl Runtime {
    /// Exports a stopped container's rootfs to a new local archive.
    ///
    /// # Errors
    /// Returns an error for a running container, missing privileges, unsupported
    /// formats, existing outputs, or failed filesystem operations.
    pub fn backup(&self, output: &Path) -> Result<()> {
        self.while_stopped_for_backup(|| self.rootfs.backup(output))
    }

    /// Exports a stopped container to a versioned migration package.
    /// Build the runtime with `ConfigEntry::load_for_export` to preserve configured values
    /// before host environment files and hostname defaults are applied.
    /// # Errors
    /// Returns errors from the lifecycle guard or package writer.
    pub fn export(&self, output: &Path) -> Result<()> {
        self.while_stopped_for_backup(|| {
            let prepared = self.rootfs.prepare()?;
            ContainerPackage::export(prepared.as_ref(), output, self.config.clone())
        })
    }

    fn while_stopped_for_backup(&self, write: impl FnOnce() -> Result<()>) -> Result<()> {
        Self::ensure_root()?;
        self.ensure_layout()?;
        let _lock = self.lock()?;
        self.scan()?;
        ensure!(
            self.state()?.is_none(),
            "container '{}' is running; stop it before backup",
            self.config.container.name
        );
        write()
    }
}
