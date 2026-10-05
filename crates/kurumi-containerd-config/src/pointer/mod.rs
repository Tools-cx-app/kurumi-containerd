use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use kurumi_containerd_error::{config::ErrorContext as _, config_ensure as ensure};

use crate::{ConfigError, Result};

/// A management display name and a resolved path to a TOML configuration.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigPointer {
    /// Display name only; does not override the TOML container identity.
    pub name: String,
    /// TOML path, resolved relative to the pointer file by `load`.
    pub file: PathBuf,
}

impl ConfigPointer {
    /// Loads `$HOME/.kurumi-containerd/config.json` using the process environment.
    ///
    /// # Errors
    /// Returns an error for missing or non-absolute HOME, or an invalid pointer.
    pub fn load_home() -> Result<Vec<Self>> {
        Self::load(&path_from_home(std::env::var_os("HOME"))?)
    }

    /// Loads a strict JSON pointer and resolves its TOML path.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or has invalid JSON or fields.
    pub fn load(path: &Path) -> Result<Vec<Self>> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read config pointer {}", path.display()))?;
        let mut pointers: Vec<Self> =
            serde_json::from_str(&source).map_err(|source| ConfigError::Context {
                context: format!("failed to parse config pointer {}", path.display()),
                source: Box::new(source),
            })?;
        ensure!(
            !pointers.is_empty(),
            "config pointer {} must not be empty",
            path.display()
        );
        let mut names = std::collections::HashSet::new();
        for pointer in &mut pointers {
            ensure!(
                !pointer.name.trim().is_empty(),
                "config pointer {}: name must not be empty",
                path.display()
            );
            ensure!(
                !pointer.file.as_os_str().to_string_lossy().trim().is_empty(),
                "config pointer {}: file must not be empty",
                path.display()
            );
            ensure!(
                names.insert(pointer.name.as_str()),
                "config pointer {}: names must be unique",
                path.display()
            );
            if pointer.file.is_relative() {
                pointer.file = path.parent().unwrap_or(Path::new(".")).join(&pointer.file);
            }
        }
        Ok(pointers)
    }

    /// Selects the only entry, or the entry matching `name`.
    ///
    /// # Errors
    /// Returns an error when the name is unknown or selection is ambiguous.
    pub fn select<'a>(pointers: &'a [Self], name: Option<&str>) -> Result<&'a Self> {
        match (pointers, name) {
            ([pointer], None) => Ok(pointer),
            (_, Some(name)) => pointers
                .iter()
                .find(|pointer| pointer.name == name)
                .with_context(|| format!("no config pointer named {name}")),
            (_, None) => Err(ConfigError::Invalid(
                "multiple config pointers require --name".to_owned(),
            )),
        }
    }
}

fn path_from_home(home: Option<OsString>) -> Result<PathBuf> {
    let home = PathBuf::from(home.context("HOME is not set; cannot locate config pointer")?);
    ensure!(home.is_absolute(), "HOME must be a nonempty absolute path");
    Ok(home.join(".kurumi-containerd/config.json"))
}

#[cfg(test)]
mod tests;
