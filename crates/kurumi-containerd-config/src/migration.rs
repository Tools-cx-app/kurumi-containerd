use std::collections::BTreeMap;
use std::fs::{File, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use kurumi_containerd_error::{config::ErrorContext as _, config_ensure as ensure};
use tempfile::{Builder, TempDir};
use uuid::Uuid;

use crate::config::{lock_document, replace_document, valid_name};
use crate::registry::{path_from_home, read_install_document, registry_for_write};
use crate::{BindMount, Config, ConfigError, Result};

pub struct ExternalPath<'a> {
    pub key: String,
    pub source: &'a Path,
    pub mount: Option<&'a BindMount>,
}

impl Config {
    /// Lists references to host files that must be rebound on the destination.
    #[must_use]
    pub fn external_paths(&self) -> Vec<ExternalPath<'_>> {
        let mut paths: Vec<_> = self
            .container
            .mounts
            .iter()
            .enumerate()
            .map(|(index, mount)| ExternalPath {
                key: format!("mount.{index}"),
                source: &mount.source,
                mount: Some(mount),
            })
            .collect();
        if let Some(source) = &self.container.environment_file {
            paths.push(ExternalPath {
                key: "environment_file".into(),
                source,
                mount: None,
            });
        }
        if let Some(binfmt) = &self.container.binfmt {
            paths.push(ExternalPath {
                key: "binfmt.interpreter".into(),
                source: &binfmt.interpreter,
                mount: None,
            });
        }
        paths
    }

    fn rebind_paths(&mut self, mappings: &[(String, PathBuf)]) -> Result<()> {
        let mut replacements = BTreeMap::new();
        for (key, path) in mappings {
            ensure!(path.is_absolute(), "{key} requires an absolute host path");
            ensure!(
                replacements.insert(key.as_str(), path).is_none(),
                "duplicate mapping: {key}"
            );
        }
        let mut replace = |key: &str, path: &mut PathBuf| -> Result<()> {
            let replacement = replacements.remove(key).with_context(|| {
                format!(
                    "missing --map {key}=/new/path for {} (use import --inspect to list paths)",
                    path.display()
                )
            })?;
            path.clone_from(replacement);
            Ok(())
        };
        for (index, mount) in self.container.mounts.iter_mut().enumerate() {
            replace(&format!("mount.{index}"), &mut mount.source)?;
        }
        if let Some(path) = &mut self.container.environment_file {
            replace("environment_file", path)?;
        }
        if let Some(binfmt) = &mut self.container.binfmt {
            replace("binfmt.interpreter", &mut binfmt.interpreter)?;
        }
        if let Some(key) = replacements.keys().next() {
            return Err(ConfigError::Invalid(format!(
                "unknown host path mapping: {key}"
            )));
        }
        Ok(())
    }
}

/// Owns a private destination until installation succeeds and the registry is published.
pub struct PendingImport {
    config: Config,
    registry: PathBuf,
    directory: TempDir,
    entry: serde_json::Value,
}

impl PendingImport {
    /// Prepares a new container under the caller's HOME with a fresh UUID.
    /// # Errors
    /// Returns errors for name collisions, invalid mappings, configuration or filesystem errors.
    pub fn prepare_home(
        config: Config,
        name: Option<&str>,
        mappings: &[(String, PathBuf)],
        image: bool,
    ) -> Result<Self> {
        Self::prepare(
            &path_from_home(std::env::var_os("HOME"))?,
            config,
            name,
            mappings,
            image,
        )
    }

    fn prepare(
        path: &Path,
        mut config: Config,
        name: Option<&str>,
        mappings: &[(String, PathBuf)],
        image: bool,
    ) -> Result<Self> {
        if let Some(name) = name {
            name.clone_into(&mut config.container.name);
        }
        ensure!(
            valid_name(&config.container.name),
            "invalid container name: {}",
            config.container.name
        );
        config.rebind_paths(mappings)?;
        let registry = registry_for_write(path)?;
        let name = &config.container.name;
        document_without_name(&registry, name)?;
        let parent = registry.parent().context("config path has no parent")?;
        let directory = Builder::new()
            .prefix(&format!("{name}-"))
            .permissions(Permissions::from_mode(0o700))
            .tempdir_in(parent)?;
        let target = directory
            .path()
            .join(if image { "rootfs.img" } else { "rootfs" });
        config.container.uuid = Some(Uuid::new_v4());
        config.container.rootfs = (!image).then(|| target.clone());
        config.container.rootfs_image = image.then_some(target);
        config.resolve_paths(&registry, true)?;
        let entry = serde_json::to_value(&config).map_err(|source| ConfigError::Context {
            context: "failed to serialize imported configuration".into(),
            source: Box::new(source),
        })?;
        config.validate_inner(false)?;
        Ok(Self {
            config,
            registry,
            directory,
            entry,
        })
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    /// Publishes the registry entry after the caller installs the rootfs.
    /// # Errors
    /// Returns errors for missing targets or a failed atomic registry write.
    pub fn commit(self) -> Result<()> {
        let container = &self.config.container;
        ensure!(
            container.rootfs.as_ref().is_some_and(|path| path.is_dir())
                || container
                    .rootfs_image
                    .as_ref()
                    .is_some_and(|path| path.is_file()),
            "imported rootfs is missing"
        );
        let _lock = lock_document(&self.registry)?;
        let mut document = document_without_name(&self.registry, &container.name)?;
        document.push(self.entry);
        replace_document(&self.registry, &document)?;
        let _ = self.directory.keep();
        File::open(
            self.registry
                .parent()
                .context("config path has no parent")?,
        )?
        .sync_all()?;
        Ok(())
    }
}

fn document_without_name(path: &Path, name: &str) -> Result<Vec<serde_json::Value>> {
    let document = read_install_document(path, true)?;
    ensure!(
        !document
            .iter()
            .any(|entry| entry["container"]["name"] == name),
        "container '{name}' already exists"
    );
    Ok(document)
}

#[cfg(test)]
#[path = "migration/tests.rs"]
mod tests;
