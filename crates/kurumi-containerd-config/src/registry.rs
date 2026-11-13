// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use crate::{Config, ConfigError, Result, config::valid_name};
use kurumi_containerd_error::{config::ErrorContext as _, config_ensure as ensure};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

/// A named container in the shared JSON configuration file.
#[derive(Debug)]
pub struct ConfigEntry {
    pub name: String,
    pub path: PathBuf,
}

impl ConfigEntry {
    /// Loads the entries under the running process's HOME.
    /// # Errors
    /// Returns an error for invalid HOME, JSON, names, or duplicate entries.
    pub fn load_home() -> Result<Vec<Self>> {
        Self::load(&path_from_home(std::env::var_os("HOME"))?)
    }

    /// Lists entries without validating individual container settings.
    /// # Errors
    /// Returns an error if the registry or its names are invalid.
    pub fn load(path: &Path) -> Result<Vec<Self>> {
        read_document(path)?
            .iter()
            .map(|entry| {
                Ok(Self {
                    name: entry["container"]["name"]
                        .as_str()
                        .context("missing container.name")?
                        .to_owned(),
                    path: path.to_owned(),
                })
            })
            .collect()
    }

    /// Selects the sole entry, or matches the exact container name.
    /// # Errors
    /// Returns an error for unknown names or ambiguous selection.
    pub fn select<'a>(entries: &'a [Self], name: Option<&str>) -> Result<&'a Self> {
        match (entries, name) {
            ([entry], None) => Ok(entry),
            (_, Some(name)) => entries
                .iter()
                .find(|entry| entry.name == name)
                .with_context(|| format!("no container named {name}")),
            (_, None) => Err(ConfigError::Invalid(
                "multiple containers require --name".to_owned(),
            )),
        }
    }

    /// Loads this container without writing the source file.
    /// # Errors
    /// Returns an error for invalid settings or missing host resources.
    pub fn load_config(&self) -> Result<Config> {
        Config::load_selected(&self.path, Some(&self.name), false)
    }

    /// Loads this container, allowing a missing rootfs target.
    /// # Errors
    /// Returns an error for invalid settings or target paths.
    pub fn load_for_install(&self) -> Result<Config> {
        Config::load_selected(&self.path, Some(&self.name), true)
    }

    /// Loads this container and atomically persists its UUID when absent.
    /// # Errors
    /// Returns an error for invalid settings or a failed persistent write.
    pub fn load_persistent(&self) -> Result<Config> {
        Config::load_persistent_selected(&self.path, Some(&self.name))
    }
}

pub(crate) fn read_document(path: &Path) -> Result<Vec<serde_json::Value>> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&source).map_err(|source| ConfigError::Context {
            context: format!("failed to parse JSON config {}", path.display()),
            source: Box::new(source),
        })?;
    ensure!(
        !entries.is_empty(),
        "config {} must not be empty",
        path.display()
    );
    let mut names = std::collections::HashSet::new();
    for entry in &entries {
        let name = entry
            .get("container")
            .and_then(|container| container.get("name"))
            .and_then(serde_json::Value::as_str)
            .context("each JSON entry requires container.name")?;
        ensure!(
            valid_name(name),
            "container name may contain only ASCII letters, digits, '.', '_' and '-' (1–128 characters, excluding '.' and '..')"
        );
        ensure!(
            names.insert(name),
            "config {}: container names must be unique",
            path.display()
        );
    }
    Ok(entries)
}

pub(crate) fn select_document(entries: &[serde_json::Value], name: Option<&str>) -> Result<usize> {
    match (entries, name) {
        ([_], None) => Ok(0),
        (_, Some(name)) => entries
            .iter()
            .position(|entry| entry["container"]["name"] == name)
            .with_context(|| format!("no container named {name}")),
        (_, None) => Err(ConfigError::Invalid(
            "multiple containers require --name".to_owned(),
        )),
    }
}

fn path_from_home(home: Option<OsString>) -> Result<PathBuf> {
    let home = PathBuf::from(home.context("HOME is not set; cannot locate configuration")?);
    ensure!(home.is_absolute(), "HOME must be a nonempty absolute path");
    Ok(home.join(".kurumi-containerd/config.json"))
}

#[cfg(test)]
#[path = "../tests/unit/registry/tests.rs"]
mod tests;
