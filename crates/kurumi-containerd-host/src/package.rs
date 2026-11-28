use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use kurumi_containerd_config::Config;
use kurumi_containerd_helper::fs::OPEN_NONBLOCK;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::archive::{publish, write_zstd};
use crate::error::{ErrorContext as _, Result, ensure};

const PAYLOAD: &str = "rootfs.tar.zst";
const MANIFEST: &str = "manifest.json";
const MANIFEST_HASH: &str = "manifest.sha256";
const MAX_MANIFEST: u64 = 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub config: Config,
    pub rootfs_sha256: String,
}

/// A container package with a verified manifest and a bounded rootfs payload.
pub struct ContainerPackage {
    file: File,
    manifest: Manifest,
    offset: u64,
    size: u64,
}

impl ContainerPackage {
    /// Streams the filesystem and its portable configuration into a new private package.
    /// # Errors
    /// Returns errors for unsafe destinations, unsupported files or failed archive writes.
    pub fn export(root: &Path, output: &Path, mut config: Config) -> Result<()> {
        config.container.rootfs = None;
        config.container.rootfs_image = None;
        config.container.uuid = None;
        publish(root, output, |root, device, file| {
            let mut archive = tar::Builder::new(file);
            let mut header = member_header(0);
            let mut entry = archive.append_writer(&mut header, PAYLOAD)?;
            let writer = write_zstd(root, device, HashWriter::new(&mut entry))?;
            let rootfs_sha256 = writer.digest();
            entry.finish()?;
            let manifest = serde_json::to_vec_pretty(&Manifest {
                version: 1,
                config,
                rootfs_sha256,
            })?;
            ensure!(
                manifest.len() as u64 <= MAX_MANIFEST,
                "package manifest exceeds 1 MiB"
            );
            let checksum = format!("{:x}", Sha256::digest(&manifest));
            archive.append_data(
                &mut member_header(manifest.len() as u64),
                MANIFEST,
                manifest.as_slice(),
            )?;
            archive.append_data(&mut member_header(64), MANIFEST_HASH, checksum.as_bytes())?;
            archive.finish()?;
            Ok(())
        })
    }

    /// Reads and checks the manifest without extracting or opening any path named in it.
    /// # Errors
    /// Returns errors for invalid layouts, metadata, checksums or unsupported versions.
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(OPEN_NONBLOCK)
            .open(path)
            .with_context(|| format!("failed to open container package {}", path.display()))?;
        ensure!(
            file.metadata()?.is_file(),
            "container package must be a regular file"
        );
        let mut archive = tar::Archive::new(&mut file);
        let mut entries = archive.entries_with_seek()?.raw(true);
        let payload = member(&mut entries, PAYLOAD)?;
        let (offset, size) = (payload.raw_file_position(), payload.size());
        ensure!(size > 0, "empty package rootfs");
        let mut metadata = member(&mut entries, MANIFEST)?;
        ensure!(
            metadata.size() <= MAX_MANIFEST,
            "package manifest exceeds 1 MiB"
        );
        let mut bytes = Vec::new();
        metadata.read_to_end(&mut bytes)?;
        let mut checksum = member(&mut entries, MANIFEST_HASH)?;
        ensure!(checksum.size() == 64, "invalid manifest checksum length");
        let mut expected = [0; 64];
        checksum.read_exact(&mut expected)?;
        ensure!(
            expected.as_slice() == format!("{:x}", Sha256::digest(&bytes)).as_bytes(),
            "package manifest SHA-256 mismatch"
        );
        ensure!(
            entries.next().transpose()?.is_none(),
            "unexpected package member"
        );
        let manifest: Manifest =
            serde_json::from_slice(&bytes).context("invalid container package manifest")?;
        ensure!(
            manifest.version == 1,
            "unsupported container package version {}",
            manifest.version
        );
        ensure!(
            manifest.rootfs_sha256.len() == 64
                && manifest
                    .rootfs_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "invalid rootfs SHA-256"
        );
        let container = &manifest.config.container;
        ensure!(
            container.rootfs.is_none()
                && container.rootfs_image.is_none()
                && container.uuid.is_none(),
            "package includes a machine-specific rootfs target or UUID"
        );
        Ok(Self {
            file,
            manifest,
            offset,
            size,
        })
    }

    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Copies the compressed payload to a private file and verifies the exact bytes to install.
    /// # Errors
    /// Returns errors for truncated or corrupt payloads and filesystem failures.
    pub fn verified_rootfs(&mut self, directory: &Path) -> Result<NamedTempFile> {
        self.file.seek(SeekFrom::Start(self.offset))?;
        let mut archive = NamedTempFile::new_in(directory)?;
        let mut writer = HashWriter::new(archive.as_file_mut());
        let copied = io::copy(
            &mut Read::by_ref(&mut self.file).take(self.size),
            &mut writer,
        )?;
        ensure!(
            copied == self.size && writer.digest() == self.manifest.rootfs_sha256,
            "package rootfs SHA-256 mismatch"
        );
        Ok(archive)
    }
}

fn member_header(size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0o600);
    header.set_size(size);
    header
}

fn member<'a, R>(entries: &mut tar::Entries<'a, R>, name: &str) -> Result<tar::Entry<'a, R>>
where
    R: Read,
{
    let entry = entries
        .next()
        .with_context(|| format!("missing package member {name}"))??;
    ensure!(
        entry.header().entry_type() == tar::EntryType::Regular
            && entry.path_bytes().as_ref() == name.as_bytes(),
        "expected regular package member {name}"
    );
    Ok(entry)
}

struct HashWriter<W> {
    writer: W,
    hash: Sha256,
}

impl<W> HashWriter<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            hash: Sha256::new(),
        }
    }

    fn digest(self) -> String {
        format!("{:x}", self.hash.finalize())
    }
}

impl<W> Write for HashWriter<W>
where
    W: Write,
{
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.writer.write(buffer)?;
        self.hash.update(&buffer[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
#[path = "package/tests.rs"]
mod tests;
