use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::AsFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    Result,
    error::{ErrorContext as _, bail, ensure},
};
use kurumi_containerd_helper::{
    fs::{
        LoopController, LoopDevice, MountFlags, mount, rename_exchange, sync_filesystem, unmount,
    },
    process::effective_uid,
};
use uuid::Uuid;

use super::Rootfs;
use crate::archive;

impl Rootfs {
    /// Extracts and validates an archive before committing the rootfs replacement.
    ///
    /// # Errors
    /// Returns errors for invalid targets, extraction, validation, or commit failures.
    pub fn install(
        &self,
        archive: &Path,
        size: Option<u64>,
        force: bool,
        validate: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            archive.is_file(),
            "rootfs archive is not a file: {}",
            archive.display()
        );
        match (&self.configured, &self.image) {
            (Some(target), None) => {
                ensure!(
                    size.is_none(),
                    "--size is only valid with container.rootfs_image"
                );
                install_directory(archive, target, force, validate)
            }
            (None, Some(target)) => {
                let size = size.context("--size is required with container.rootfs_image")?;
                install_image(archive, target, size, &self.mountpoint, force, validate)
            }
            _ => bail!("configure exactly one rootfs target"),
        }
    }
}

fn install_directory(
    archive_path: &Path,
    target: &Path,
    force: bool,
    validate: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    ensure_trusted_parent(target)?;
    reject_unsafe_target(target, force, true)?;
    let staged = sibling(target, "install");
    fs::create_dir(&staged)
        .with_context(|| format!("failed to create temporary rootfs {}", staged.display()))?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o700))?;
    let result = (|| {
        archive::extract(archive_path, &staged)?;
        validate(&staged)?;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
        commit(&staged, target, force)
    })();
    if staged.exists() {
        let _ = fs::remove_dir_all(&staged);
    }
    result
}

fn install_image(
    archive_path: &Path,
    target: &Path,
    size: u64,
    mount_base: &Path,
    force: bool,
    validate: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    ensure!(size > 0, "rootfs image size must be greater than zero");
    ensure_trusted_parent(target)?;
    reject_unsafe_target(target, force, false)?;
    let staged = sibling(target, "install");
    let result = (|| {
        let image = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged)?;
        image.set_len(size)?;
        image.sync_all()?;
        format_ext4(&staged)?;

        let mountpoint = mount_base.with_extension(format!("install-{}", Uuid::new_v4()));
        let mut mounted = MountedImage::mount(&staged, &mountpoint)?;
        archive::extract(archive_path, mounted.path())?;
        validate(mounted.path())?;
        mounted.finish()?;
        commit(&staged, target, force)
    })();
    if staged.exists() {
        let _ = fs::remove_file(&staged);
    }
    result
}

fn format_ext4(path: &Path) -> Result<()> {
    let formatter = [
        "/usr/sbin/mke2fs",
        "/sbin/mke2fs",
        "/system/bin/mke2fs",
        "/usr/sbin/mkfs.ext4",
        "/sbin/mkfs.ext4",
        "/system/bin/mkfs.ext4",
    ]
    .into_iter()
    .find(|candidate| Path::new(candidate).is_file())
    .context("mke2fs or mkfs.ext4 was not found")?;
    let output = Command::new(formatter)
        .args(["-q", "-F", "-t", "ext4"])
        .arg(path)
        .env_clear()
        .output()
        .context("failed to execute ext4 formatter")?;
    ensure!(
        output.status.success(),
        "ext4 formatter failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

struct MountedImage {
    path: PathBuf,
    loop_device: Option<LoopDevice>,
    mounted: bool,
}

impl MountedImage {
    fn mount(image: &Path, path: &Path) -> Result<Self> {
        fs::create_dir_all(path)?;
        let mut device = LoopController::open()
            .context("failed to open /dev/loop-control")?
            .attach(image)
            .context("failed to attach temporary rootfs image")?;
        if let Err(error) = mount(
            Some(device.path()),
            path,
            Some("ext4"),
            MountFlags::NOATIME | MountFlags::NODIRATIME,
            None,
        ) {
            let _ = device.clear();
            let _ = fs::remove_dir(path);
            return Err(error).context("failed to mount temporary rootfs image");
        }
        Ok(Self {
            path: path.to_path_buf(),
            loop_device: Some(device),
            mounted: true,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn finish(&mut self) -> Result<()> {
        let root = File::open(&self.path)?;
        sync_filesystem(root.as_fd())?;
        unmount(&self.path, false).context("failed to unmount temporary rootfs image")?;
        self.mounted = false;
        if let Some(device) = self.loop_device.take() {
            let mut device = device;
            device.clear()?;
        }
        fs::remove_dir(&self.path)?;
        Ok(())
    }
}

impl Drop for MountedImage {
    fn drop(&mut self) {
        if self.mounted {
            let _ = unmount(&self.path, true);
        }
        self.loop_device.take();
        let _ = fs::remove_dir(&self.path);
    }
}

fn reject_unsafe_target(target: &Path, force: bool, directory: bool) -> Result<()> {
    if let Ok(metadata) = target.symlink_metadata() {
        ensure!(
            !metadata.file_type().is_symlink(),
            "rootfs target cannot be a symlink"
        );
        ensure!(
            if directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            },
            "existing rootfs target has the wrong file type: {}",
            target.display()
        );
        ensure!(force, "rootfs target already exists: {}", target.display());
    }
    Ok(())
}

fn commit(staged: &Path, target: &Path, force: bool) -> Result<()> {
    let parent = target.parent().context("rootfs target has no parent")?;
    if !target.exists() {
        fs::rename(staged, target)
            .with_context(|| format!("failed to install rootfs at {}", target.display()))?;
        return File::open(parent)?.sync_all().map_err(Into::into);
    }
    ensure!(force, "rootfs target already exists: {}", target.display());
    rename_exchange(staged, target).context("failed to atomically replace rootfs")?;
    File::open(parent)?.sync_all()?;
    if let Err(error) = remove_path(staged) {
        tracing::warn!(path = %staged.display(), %error, "failed to remove replaced rootfs");
    } else {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<()> {
    if path.symlink_metadata()?.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn sibling(target: &Path, kind: &str) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{name}.{kind}-{}", Uuid::new_v4()))
}

fn ensure_trusted_parent(target: &Path) -> Result<()> {
    let parent = target.parent().context("rootfs target has no parent")?;
    let metadata = fs::symlink_metadata(parent)?;
    ensure!(
        metadata.is_dir(),
        "rootfs parent is not a directory: {}",
        parent.display()
    );
    ensure!(
        !metadata.file_type().is_symlink(),
        "rootfs parent must not be a symlink"
    );
    ensure!(
        metadata.uid() == effective_uid(),
        "rootfs parent has an untrusted owner"
    );
    ensure!(
        metadata.mode() & 0o022 == 0,
        "rootfs parent must not be group/world writable"
    );
    for ancestor in parent.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(ancestor)?;
        ensure!(
            metadata.mode() & 0o022 == 0 || metadata.mode() & 0o1000 != 0,
            "rootfs ancestor is writable by an untrusted user: {}",
            ancestor.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
