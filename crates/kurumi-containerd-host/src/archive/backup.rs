// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File, Metadata, OpenOptions},
    io::{BufWriter, Write, copy},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

use kurumi_containerd_helper::fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW};
use tempfile::Builder;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::ensure_safe_link;
use crate::{
    Result,
    error::{ErrorContext as _, bail, ensure},
};

pub(crate) fn backup(root: &Path, output: &Path) -> Result<()> {
    let extension = output.extension().and_then(|value| value.to_str());
    ensure!(
        matches!(extension, Some("tar" | "xz" | "zst" | "zip")),
        "unsupported backup extension; expected .tar, .xz, .zst, or .zip"
    );
    let root = root.canonicalize().context("failed to resolve rootfs")?;
    let metadata = fs::metadata(&root)?;
    ensure!(metadata.is_dir(), "rootfs must be a directory");
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .context("failed to resolve backup output directory")?;
    let output = parent.join(
        output
            .file_name()
            .context("backup output has no filename")?,
    );
    ensure!(
        !output.starts_with(&root),
        "backup output must be outside rootfs"
    );
    match output.symlink_metadata() {
        Ok(_) => bail!("backup output already exists: {}", output.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to inspect backup output"),
    }
    ensure_no_mounts(&root)?;

    let mut temporary = Builder::new()
        .prefix(".kurumi-backup-")
        .tempfile_in(&parent)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    {
        let writer = BufWriter::new(temporary.as_file_mut());
        match extension {
            Some("zip") => write_zip(&root, metadata.dev(), writer)?,
            Some("tar") => write_tar(&root, metadata.dev(), writer)?.flush()?,
            Some("xz") => {
                let encoder = xz2::write::XzEncoder::new(writer, 6);
                write_tar(&root, metadata.dev(), encoder)?
                    .finish()?
                    .flush()?;
            }
            Some("zst") => {
                let encoder = zstd::stream::write::Encoder::new(writer, 0)?;
                write_tar(&root, metadata.dev(), encoder)?
                    .finish()?
                    .flush()?;
            }
            _ => unreachable!("extension was validated"),
        }
    }
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(&output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish backup {}", output.display()))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn ensure_no_mounts(root: &Path) -> Result<()> {
    let mounts = procfs::process::Process::myself()?.mountinfo()?;
    let mut covering = None;
    for mount in &mounts {
        // procfs leaves mountinfo's octal path escapes intact.
        let point = unescape_mount_path(&mount.mount_point)?;
        ensure!(
            point == root || !point.starts_with(root),
            "unmount rootfs subtree before backup: {}",
            point.display()
        );
        if root.starts_with(&point)
            && covering
                .as_ref()
                .is_none_or(|(length, _)| point.as_os_str().len() > *length)
        {
            covering = Some((point.as_os_str().len(), mount.fs_type.as_str()));
        }
    }
    ensure!(
        !matches!(
            covering.map(|(_, kind)| kind),
            Some(
                "proc"
                    | "sysfs"
                    | "devtmpfs"
                    | "devpts"
                    | "cgroup"
                    | "cgroup2"
                    | "securityfs"
                    | "debugfs"
                    | "tracefs"
            )
        ),
        "cannot back up a live kernel filesystem"
    );
    Ok(())
}

fn unescape_mount_path(path: &Path) -> Result<PathBuf> {
    let bytes = path.as_os_str().as_bytes();
    if !bytes.contains(&b'\\') {
        return Ok(path.to_path_buf());
    }
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            let digits = bytes
                .get(index + 1..index + 4)
                .context("invalid mountinfo path escape")?;
            ensure!(
                digits.iter().all(|byte| matches!(byte, b'0'..=b'7')),
                "invalid mountinfo path escape"
            );
            let value = u16::from(digits[0] - b'0') * 64
                + u16::from(digits[1] - b'0') * 8
                + u16::from(digits[2] - b'0');
            decoded.push(u8::try_from(value).context("invalid mountinfo path escape")?);
            index += 4;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Ok(PathBuf::from(OsString::from_vec(decoded)))
}

fn walk(
    root: &Path,
    relative: &Path,
    device: u64,
    visit: &mut impl FnMut(&Path, &Path, &Metadata) -> Result<()>,
) -> Result<()> {
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(
        metadata.dev() == device,
        "rootfs filesystem changed at {}",
        path.display()
    );
    let kind = metadata.file_type();
    ensure!(
        kind.is_file() || kind.is_dir() || kind.is_symlink(),
        "unsupported rootfs special file: {}",
        path.display()
    );
    visit(&path, relative, &metadata)?;
    if kind.is_dir() {
        for entry in fs::read_dir(&path)? {
            walk(root, &relative.join(entry?.file_name()), device, visit)?;
        }
    }
    Ok(())
}

fn open_file(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_NOFOLLOW | OPEN_CLOEXEC)
        .open(path)?)
}

fn write_tar<W: Write>(root: &Path, device: u64, writer: W) -> Result<W> {
    let mut archive = tar::Builder::new(writer);
    archive.follow_symlinks(false);
    let mut hardlinks = HashMap::new();
    walk(
        root,
        Path::new(""),
        device,
        &mut |path, relative, metadata| {
            let attributes = xattrs(path)?;
            archive.append_pax_extensions(
                attributes
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_slice())),
            )?;
            let name = if relative.as_os_str().is_empty() {
                Path::new(".")
            } else {
                relative
            };
            if metadata.is_file() {
                let identity = (metadata.dev(), metadata.ino());
                if let Some(target) = hardlinks.get(&identity) {
                    let mut header = tar::Header::new_gnu();
                    header.set_metadata(metadata);
                    header.set_entry_type(tar::EntryType::Link);
                    header.set_size(0);
                    archive.append_link(&mut header, name, target)?;
                } else {
                    archive.append_file(name, &mut open_file(path)?)?;
                    if metadata.nlink() > 1 {
                        hardlinks.insert(identity, name.to_path_buf());
                    }
                }
            } else {
                archive.append_path_with_name(path, name)?;
            }
            Ok(())
        },
    )?;
    archive.finish()?;
    Ok(archive.into_inner()?)
}

fn write_zip<W: Write + std::io::Seek>(root: &Path, device: u64, writer: W) -> Result<()> {
    tracing::warn!(
        "ZIP backup does not preserve ownership, timestamps, xattrs, special permission bits, or hardlink identity; use tar, .xz, or .zst for metadata preservation"
    );
    let mut archive = ZipWriter::new(writer);
    walk(
        root,
        Path::new(""),
        device,
        &mut |path, relative, metadata| {
            if relative.as_os_str().is_empty() {
                return Ok(());
            }
            let name = relative
                .to_str()
                .context("ZIP requires UTF-8 rootfs paths")?;
            ensure!(
                relative.components().all(|component| {
                    let value = component.as_os_str().as_bytes();
                    !(value.contains(&b'\\')
                        || value.len() >= 2 && value[0].is_ascii_alphabetic() && value[1] == b':')
                }),
                "ZIP cannot represent this rootfs path without normalization: {}",
                relative.display()
            );
            let options = SimpleFileOptions::default()
                .unix_permissions(metadata.mode())
                .large_file(metadata.len() >= u64::from(u32::MAX));
            if metadata.is_dir() {
                archive.add_directory(name, options)?;
            } else if metadata.file_type().is_symlink() {
                let link = fs::read_link(path)?;
                ensure_safe_link(relative, &link)?;
                archive.add_symlink(
                    name,
                    link.to_str()
                        .context("ZIP requires UTF-8 symlink targets")?,
                    options,
                )?;
            } else {
                archive.start_file(name, options)?;
                copy(&mut open_file(path)?, &mut archive)?;
            }
            Ok(())
        },
    )?;
    archive.finish()?.flush()?;
    Ok(())
}

fn xattrs(path: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let names = match xattr::list(path) {
        Ok(names) => names,
        Err(error) if error.raw_os_error() == Some(libc::ENOTSUP) => return Ok(Vec::new()),
        Err(error) => return Err(error).context("failed to list rootfs extended attributes"),
    };
    let mut attributes = Vec::new();
    for name in names {
        let key = format!(
            "SCHILY.xattr.{}",
            name.to_str().context("non-UTF-8 extended attribute name")?
        );
        ensure!(
            !key.contains(['=', '\n']),
            "extended attribute name cannot be represented in tar"
        );
        let value = xattr::get(path, &name)?
            .context("rootfs extended attribute disappeared during backup")?;
        attributes.push((key, value));
    }
    Ok(attributes)
}
