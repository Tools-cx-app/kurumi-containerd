use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, copy},
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::{Component, Path},
};

use crate::{
    Result,
    error::{ErrorContext as _, bail, ensure},
};
use flate2::read::GzDecoder;
use kurumi_containerd_helper::{
    fs::{OPEN_CLOEXEC, OPEN_NOFOLLOW},
    process::effective_uid,
};
use tar::Archive;
use xz2::read::XzDecoder;
use zip::ZipArchive;
use zstd::stream::read::Decoder as ZstdDecoder;

const GZIP_MAGIC: &[u8] = &[0x1f, 0x8b];
const XZ_MAGIC: &[u8] = &[0xfd, b'7', b'z', b'X', b'Z', 0x00];
const ZSTD_MAGIC: &[u8] = &[0x28, 0xb5, 0x2f, 0xfd];
const ZIP_LOCAL_FILE_MAGIC: &[u8] = b"PK\x03\x04";
const ZIP_EMPTY_ARCHIVE_MAGIC: &[u8] = b"PK\x05\x06";
const ZIP_SPANNED_ARCHIVE_MAGIC: &[u8] = b"PK\x07\x08";

pub(crate) fn extract(archive: &Path, target: &Path) -> Result<()> {
    let mut file = File::open(archive)
        .with_context(|| format!("failed to open rootfs archive {}", archive.display()))?;

    match archive_kind(&mut file)? {
        ArchiveKind::Tar => extract_tar(file, target),
        ArchiveKind::Gzip => extract_tar(GzDecoder::new(file), target),
        ArchiveKind::Xz => extract_tar(XzDecoder::new(file), target),
        ArchiveKind::Zstd => extract_tar(ZstdDecoder::new(file)?, target),
        ArchiveKind::Zip => extract_zip(file, target),
    }
}

#[derive(Clone, Copy)]
enum ArchiveKind {
    Tar,
    Gzip,
    Xz,
    Zstd,
    Zip,
}

fn archive_kind(file: &mut File) -> Result<ArchiveKind> {
    let mut header = [0_u8; 512];
    let read = file.read(&mut header)?;
    file.seek(SeekFrom::Start(0))?;
    let header = &header[..read];

    if header.starts_with(GZIP_MAGIC) {
        Ok(ArchiveKind::Gzip)
    } else if header.starts_with(XZ_MAGIC) {
        Ok(ArchiveKind::Xz)
    } else if header.starts_with(ZSTD_MAGIC) {
        Ok(ArchiveKind::Zstd)
    } else if header.starts_with(ZIP_LOCAL_FILE_MAGIC)
        || header.starts_with(ZIP_EMPTY_ARCHIVE_MAGIC)
        || header.starts_with(ZIP_SPANNED_ARCHIVE_MAGIC)
    {
        Ok(ArchiveKind::Zip)
    } else if valid_tar_header(header) {
        Ok(ArchiveKind::Tar)
    } else {
        bail!("unsupported rootfs archive format")
    }
}

fn valid_tar_header(header: &[u8]) -> bool {
    if header.len() < 512 {
        return false;
    }
    let Ok(checksum) = std::str::from_utf8(&header[148..156]) else {
        return false;
    };
    let Ok(checksum) = u32::from_str_radix(checksum.trim_matches(['\0', ' ']), 8) else {
        return false;
    };
    let actual = header[..148]
        .iter()
        .chain([b' '; 8].iter())
        .chain(header[156..].iter())
        .map(|byte| u32::from(*byte))
        .sum::<u32>();
    checksum == actual
}

fn extract_tar(reader: impl Read, target: &Path) -> Result<()> {
    let mut archive = Archive::new(reader);
    archive.set_preserve_permissions(true);
    archive.set_preserve_ownerships(effective_uid() == 0);
    archive.set_unpack_xattrs(true);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file()
                || kind.is_dir()
                || kind.is_symlink()
                || kind.is_hard_link()
                || kind.is_contiguous()
                || kind.is_gnu_sparse(),
            "unsupported tar entry type {:?}: {}",
            kind,
            entry.path()?.display()
        );
        ensure!(
            entry.unpack_in(target)?,
            "tar entry has an unsafe path: {}",
            entry.path()?.display()
        );
    }
    Ok(())
}

fn extract_zip(reader: impl Read + Seek, target: &Path) -> Result<()> {
    let mut archive = ZipArchive::new(reader).context("failed to open ZIP archive")?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .context("ZIP entry has an unsafe path")?;
        ensure_safe_path(target, &relative)?;
        let output = target.join(&relative);
        let mode = entry
            .unix_mode()
            .unwrap_or(if entry.is_dir() { 0o755 } else { 0o644 });

        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o7777))?;
        } else if mode & 0o170_000 == 0o120_000 {
            let mut link = String::new();
            entry.read_to_string(&mut link)?;
            ensure_safe_link(&relative, Path::new(&link))?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            symlink(link, output)?;
        } else {
            ensure!(
                mode & 0o170_000 == 0 || mode & 0o170_000 == 0o100_000,
                "unsupported ZIP special entry: {}",
                relative.display()
            );
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .custom_flags(OPEN_NOFOLLOW | OPEN_CLOEXEC)
                .open(&output)?;
            copy(&mut entry, &mut file)?;
            fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o7777))?;
        }
    }
    Ok(())
}

fn ensure_safe_path(root: &Path, relative: &Path) -> Result<()> {
    ensure!(
        !relative.is_absolute()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_) | Component::CurDir)),
        "archive entry has an unsafe path: {}",
        relative.display()
    );
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if let Component::Normal(component) = component {
            current.push(component);
            match current.symlink_metadata() {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("archive entry traverses a symlink: {}", relative.display());
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to inspect {}", current.display()));
                }
            }
        }
    }
    Ok(())
}

fn ensure_safe_link(entry: &Path, link: &Path) -> Result<()> {
    ensure!(!link.is_absolute(), "ZIP symlink target must be relative");
    let mut depth = entry
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in link.components() {
        match component {
            Component::ParentDir => {
                ensure!(depth > 0, "ZIP symlink target escapes rootfs");
                depth -= 1;
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                bail!("ZIP symlink target escapes rootfs");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
