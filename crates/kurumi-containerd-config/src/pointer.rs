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
                names.insert(pointer.name.clone()),
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
mod tests {
    use std::{error::Error as _, fs, path::Path};

    use super::*;
    use crate::Config;

    #[test]
    fn selects_named_entry_and_rejects_ambiguous_lists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(
            &path,
            r#"[{"name":"one","file":"one.toml"},{"name":"two","file":"two.toml"}]"#,
        )
        .unwrap();
        let pointers = ConfigPointer::load(&path).unwrap();
        assert!(ConfigPointer::select(&pointers, None).is_err());
        assert!(ConfigPointer::select(&pointers, Some("missing")).is_err());
        assert_eq!(
            ConfigPointer::select(&pointers, Some("two")).unwrap().file,
            dir.path().join("two.toml")
        );
        for source in [
            "[]",
            r#"{"name":"one","file":"one.toml"}"#,
            r#"[{"name":"one","file":"one.toml"},{"name":"one","file":"two.toml"}]"#,
        ] {
            fs::write(&path, source).unwrap();
            assert!(ConfigPointer::load(&path).is_err());
        }
    }

    #[test]
    fn resolves_pointer_without_overriding_toml_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::create_dir(dir.path().join("configs")).unwrap();
        fs::write(
            dir.path().join("configs/container.toml"),
            "[runtime]\n[container]\nname='runtime-name'\nrootfs='rootfs'\n",
        )
        .unwrap();
        fs::write(
            &path,
            r#"[{"name":"Display name","file":"configs/container.toml"}]"#,
        )
        .unwrap();
        let pointers = ConfigPointer::load(&path).unwrap();
        let pointer = ConfigPointer::select(&pointers, None).unwrap();
        assert_eq!(pointer.name, "Display name");
        assert_eq!(pointer.file, dir.path().join("configs/container.toml"));
        let config = Config::load_for_install(&pointer.file).unwrap();
        assert_eq!(config.container.name, "runtime-name");
        assert_eq!(
            config.container.rootfs,
            Some(dir.path().join("configs/rootfs"))
        );
        fs::write(
            &path,
            r#"[{"name":" name ","file":"/absolute/config.toml"}]"#,
        )
        .unwrap();
        let pointers = ConfigPointer::load(&path).unwrap();
        let pointer = ConfigPointer::select(&pointers, Some(" name ")).unwrap();
        assert_eq!(pointer.name, " name ");
        assert_eq!(pointer.file, Path::new("/absolute/config.toml"));
    }

    #[test]
    fn rejects_invalid_pointer_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        for source in [
            "{}",
            "null",
            "[]",
            r#"{"name":"n"}"#,
            r#"{"file":"f"}"#,
            r#"{"name":1,"file":"f"}"#,
            r#"{"name":"n","file":null}"#,
            r#"{"name":"n","file":"f","extra":true}"#,
            r#"{"name":"","file":"f"}"#,
            r#"{"name":"  ","file":"f"}"#,
            r#"{"name":"n","file":""}"#,
            r#"{"name":"n","file":"\t"}"#,
        ] {
            fs::write(&path, format!("[{source}]")).unwrap();
            let error = ConfigPointer::load(&path).unwrap_err();
            assert!(
                error.to_string().contains(&path.display().to_string()),
                "{error}"
            );
        }
        fs::write(&path, "{").unwrap();
        assert!(ConfigPointer::load(&path).unwrap_err().source().is_some());
        fs::remove_file(&path).unwrap();
        assert!(ConfigPointer::load(&path).unwrap_err().source().is_some());
    }

    #[test]
    fn requires_absolute_nonempty_home() {
        for home in [None, Some("".into()), Some("relative".into())] {
            assert!(path_from_home(home).is_err());
        }
        assert_eq!(
            path_from_home(Some("/home/user".into())).unwrap(),
            Path::new("/home/user/.kurumi-containerd/config.json")
        );
    }
}
