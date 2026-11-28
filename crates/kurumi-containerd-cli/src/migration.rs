use std::path::{Path, PathBuf};

use kurumi_containerd_config::{ConfigEntry, PendingImport};
use kurumi_containerd_daemon::{Client, Operation, Request, Response};
use kurumi_containerd_error::{Result, ensure};
use kurumi_containerd_host::package::{ContainerPackage, Manifest};

use crate::output;

pub(super) fn parse_mapping(value: &str) -> Result<(String, PathBuf), String> {
    let (key, path) = value
        .split_once('=')
        .ok_or("expected KEY=/absolute/host/path")?;
    if key.is_empty() || !Path::new(path).is_absolute() {
        return Err("expected KEY=/absolute/host/path".into());
    }
    Ok((key.to_owned(), path.into()))
}

pub(super) fn export(client: &Client, name: &str, destination: &Path) -> Result<()> {
    let entries = ConfigEntry::load_home()?;
    let config = ConfigEntry::select(&entries, Some(name))?.load_for_export()?;
    let response = client.request(&Request {
        config: Some(config),
        operation: Operation::Export {
            output: std::path::absolute(destination)?,
        },
    })?;
    ensure!(
        matches!(response, Response::Done),
        "unexpected export response: {response:?}"
    );
    output::write(|out| writeln!(out, "Exported {name} to {}", destination.display()))?;
    Ok(())
}

pub(super) fn import(
    client: &Client,
    path: &Path,
    name: Option<&str>,
    inspect: bool,
    mappings: &[(String, PathBuf)],
    size: Option<u64>,
) -> Result<()> {
    let mut package = ContainerPackage::open(path)?;
    describe(package.manifest())?;
    if inspect {
        return Ok(());
    }
    let pending = PendingImport::prepare_home(
        package.manifest().config.clone(),
        name,
        mappings,
        size.is_some(),
    )?;
    let payload = package.verified_rootfs(pending.directory())?;
    let config = pending.config().clone();
    let name = config.container.name.clone();
    let response = client.request(&Request {
        config: Some(config),
        operation: Operation::Install {
            archive: payload.path().to_path_buf(),
            size,
            force: false,
        },
    })?;
    ensure!(
        matches!(response, Response::Done),
        "unexpected install response: {response:?}"
    );
    payload.close()?;
    pending.commit()?;
    output::write(|out| writeln!(out, "Imported {name} (stopped)"))?;
    Ok(())
}

fn describe(manifest: &Manifest) -> Result<()> {
    output::write(|out| {
        writeln!(
            out,
            "Container: {} (package v{})",
            manifest.config.container.name, manifest.version
        )?;
        writeln!(out, "Rootfs SHA-256: {}", manifest.rootfs_sha256)?;
        let paths = manifest.config.external_paths();
        if paths.is_empty() {
            writeln!(out, "External host paths: none")?;
        }
        for path in paths {
            write!(out, "{}: {}", path.key, path.source.display())?;
            if let Some(mount) = path.mount {
                write!(
                    out,
                    " -> {} ({})",
                    mount.target.display(),
                    if mount.read_only { "ro" } else { "rw" }
                )?;
            }
            writeln!(out)?;
        }
        Ok(())
    })?;
    Ok(())
}
