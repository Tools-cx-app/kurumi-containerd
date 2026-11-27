// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fs::{self, File},
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use clap::Args;
use tempfile::NamedTempFile;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{build::ANDROID_ABIS, validate_version, workspace};

#[derive(Args)]
pub struct ModuleArgs {
    /// Artifact root containing kurumi-containerd-android-<abi>/kurumi-containerd.
    #[arg(long)]
    pub binaries: PathBuf,
    /// Destination directory, relative to the workspace root.
    #[arg(long, default_value = "dist")]
    pub output: PathBuf,
    /// Module version; defaults to the workspace version.
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    pub version: String,
    /// Positive Android versionCode (at most 2147483647).
    #[arg(long, value_parser = clap::value_parser!(i32).range(1..))]
    pub version_code: i32,
}

pub fn package(args: &ModuleArgs) -> Result<()> {
    validate_version(&args.version)?;
    ensure!(args.version_code > 0, "version-code must be positive");
    let binaries = workspace().join(&args.binaries);
    let mut payloads = Vec::new();
    for abi in ANDROID_ABIS {
        let binary = binaries
            .join(format!("kurumi-containerd-android-{abi}"))
            .join("kurumi-containerd");
        ensure!(
            binary.is_file() && binary.metadata()?.len() > 0,
            "Missing or empty binary: {}",
            binary.display()
        );
        payloads.push((abi, binary));
    }
    let output = workspace().join(&args.output);
    fs::create_dir_all(&output)?;
    let temporary = NamedTempFile::new_in(&output)?;
    let mut archive = ZipWriter::new(temporary.reopen()?);
    archive.start_file("module.prop", options(0o644))?;
    write!(
        archive,
        "id=kurumi-containerd\nname=KurumiContainerd\nversion={}\nversionCode={}\n\
         author=KurumiContainerd contributors\n\
         description=Linux containers with configurable boot startup\n",
        args.version, args.version_code
    )?;
    let module = workspace().join("android/module");
    append_directory(&mut archive, &module, &module)?;
    for (abi, binary) in payloads {
        append_file(
            &mut archive,
            &binary,
            &format!("binaries/{abi}/kurumi-containerd"),
            0o755,
        )?;
    }
    for document in ["LICENSE", "config.example.json"] {
        append_file(&mut archive, &workspace().join(document), document, 0o644)?;
    }
    archive.finish()?;
    let destination = output.join(format!(
        "kurumi-containerd-{}-android-module.zip",
        args.version
    ));
    temporary.persist(&destination)?;
    println!("{}", destination.display());
    Ok(())
}

fn options(mode: u32) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(9))
        .unix_permissions(mode)
}

fn append_directory(archive: &mut ZipWriter<File>, root: &Path, directory: &Path) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            append_directory(archive, root, &path)?;
        } else if metadata.is_file() {
            let name = path
                .strip_prefix(root)?
                .to_str()
                .context("Non-UTF-8 module path")?;
            append_file(archive, &path, name, metadata.permissions().mode() & 0o777)?;
        }
    }
    Ok(())
}

fn append_file(archive: &mut ZipWriter<File>, path: &Path, name: &str, mode: u32) -> Result<()> {
    archive.start_file(name, options(mode))?;
    io::copy(&mut File::open(path)?, archive)?;
    Ok(())
}
