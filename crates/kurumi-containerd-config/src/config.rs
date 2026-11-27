// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, chown},
    path::{Component, Path, PathBuf},
};

use fs2::FileExt;
use kurumi_containerd_error::{config::ErrorContext as _, config_bail as bail, config_ensure as ensure};
pub use kurumi_containerd_error::config::{ConfigError, Result};
use uuid::Uuid;

use crate::{
    BinfmtConfig, Config, NetworkMode, Protocol, Validate, environment::valid_env_key,
    parse_environment,
};

impl Config {
    /// Loads, resolves, and validates a JSON configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed, a host path
    /// cannot be resolved, or a configuration invariant is violated.
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_selected(path, None, false)
    }

    /// Loads configuration for installing a rootfs into a target that may not
    /// exist yet.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration is invalid or a target parent
    /// cannot be resolved.
    pub fn load_for_install(path: &Path) -> Result<Self> {
        Self::load_selected(path, None, true)
    }

    pub(crate) fn load_selected(path: &Path, name: Option<&str>, installing: bool) -> Result<Self> {
        let mut document = crate::registry::read_document(path)?;
        let index = crate::registry::select_document(&document, name)?;
        let mut config: Self =
            serde_json::from_value(document.swap_remove(index)).map_err(|source| {
                ConfigError::Context {
                    context: format!("failed to parse JSON config {}", path.display()),
                    source: Box::new(source),
                }
            })?;
        config.resolve_paths(path, installing)?;
        config.validate_inner(!installing)?;
        Ok(config)
    }

    /// Loads a configuration and persists an external identity when it does
    /// not already have one. The write is atomic so a killed CLI cannot leave
    /// a partially written JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error when the source configuration is invalid or the
    /// persistent JSON rewrite cannot be committed.
    pub fn load_persistent(path: &Path) -> Result<Self> {
        Self::load_persistent_selected(path, None)
    }

    pub(crate) fn load_persistent_selected(path: &Path, name: Option<&str>) -> Result<Self> {
        let persistent_path = path
            .canonicalize()
            .with_context(|| format!("failed to resolve config {}", path.display()))?;
        let _lock = lock_document(&persistent_path)?;
        let config = Self::load_selected(path, name, false)?;
        if config.container.uuid.is_none() {
            let mut document = crate::registry::read_document(&persistent_path)?;
            let index = crate::registry::select_document(&document, name)?;
            document[index]["container"]["uuid"] =
                serde_json::Value::String(Uuid::new_v4().to_string());
            write_document(&persistent_path, &document)?;
            return Self::load_selected(path, name, false);
        }
        Ok(config)
    }

    fn resolve_paths(&mut self, config_path: &Path, installing: bool) -> Result<()> {
        let base = config_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if let Some(rootfs) = &mut self.container.rootfs {
            *rootfs = if installing {
                absolute_install_target(base, rootfs)?
            } else {
                absolute_from(base, rootfs)?
            };
        }
        if let Some(image) = &mut self.container.rootfs_image {
            *image = if installing {
                absolute_install_target(base, image)?
            } else {
                absolute_from(base, image)?
            };
        }
        for mount in &mut self.container.mounts {
            mount.source = absolute_from(base, &mount.source)?;
        }
        if let Some(environment_file) = &mut self.container.environment_file {
            *environment_file = absolute_from(base, environment_file)?;
        }
        if let Some(binfmt) = &mut self.container.binfmt {
            binfmt.interpreter = absolute_from(base, &binfmt.interpreter)?;
        }
        Ok(())
    }
}

impl Config {
    /// Validates and normalizes values that affect namespace and mount safety.
    ///
    /// This also resolves the default hostname and merges an environment file
    /// into the inline environment, so callers must expect mutation and I/O.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid names or paths, missing rootfs content,
    /// invalid networking values, or malformed environment keys.
    #[allow(clippy::too_many_lines)]
    fn validate_inner(&mut self, require_rootfs: bool) -> Result<()> {
        ensure!(
            valid_name(&self.container.name),
            "container name may contain only ASCII letters, digits, '.', '_' and '-'"
        );
        ensure!(
            self.container.rootfs.is_some() ^ self.container.rootfs_image.is_some(),
            "configure exactly one of container.rootfs or container.rootfs_image"
        );
        if require_rootfs && let Some(rootfs) = &self.container.rootfs {
            ensure!(
                rootfs.is_absolute(),
                "container.rootfs must resolve to an absolute path"
            );
            ensure!(
                rootfs.is_dir(),
                "rootfs is not a directory: {}",
                rootfs.display()
            );
        }
        if require_rootfs && let Some(image) = &self.container.rootfs_image {
            ensure!(
                image.is_absolute(),
                "container.rootfs_image must resolve to an absolute path"
            );
            ensure!(
                image.is_file() || image.exists(),
                "rootfs image does not exist: {}",
                image.display()
            );
        }
        ensure!(
            self.container.init.is_absolute(),
            "container.init must be an absolute path inside rootfs"
        );
        if let Some(binfmt) = &self.container.binfmt {
            binfmt.validate()?;
        }
        ensure!(
            self.runtime.stop_timeout_seconds > 0,
            "runtime.stop_timeout_seconds must be greater than zero"
        );
        if self.container.hostname.is_empty() {
            self.container.hostname.clone_from(&self.container.name);
        }
        ensure!(
            self.container.network_options.prefix <= 32,
            "network prefix must be <= 32"
        );
        ensure!(
            valid_interface_name(&self.container.network_options.bridge),
            "invalid NAT bridge name"
        );
        if matches!(
            self.container.network,
            NetworkMode::Gateway | NetworkMode::Dhcp
        ) {
            ensure!(
                valid_interface_name(&self.container.network_options.gateway_bridge),
                "gateway and dhcp modes require a valid network_options.gateway_bridge"
            );
        }
        for dns in &self.container.network_options.dns {
            ensure!(
                dns.parse::<std::net::IpAddr>().is_ok(),
                "invalid DNS address: {dns}"
            );
        }
        for port in &self.container.network_options.ports {
            ensure!(
                port.host > 0 && port.container > 0,
                "forwarded ports must be non-zero"
            );
        }
        for (index, port) in self.container.network_options.ports.iter().enumerate() {
            for other in &self.container.network_options.ports[index + 1..] {
                if port.protocol != other.protocol {
                    continue;
                }
                ensure!(
                    port.host != other.host,
                    "duplicate host port {}/{}",
                    port.host,
                    protocol_name(port.protocol)
                );
                ensure!(
                    port.container != other.container,
                    "duplicate container port {}/{}",
                    port.container,
                    protocol_name(port.protocol)
                );
            }
        }
        if let Some(memory) = self.container.resources.memory_bytes {
            ensure!(
                memory >= 4 * 1024 * 1024,
                "memory_bytes must be at least 4194304"
            );
        }
        match (
            self.container.resources.cpu_quota,
            self.container.resources.cpu_period,
        ) {
            (Some(quota), Some(period)) => {
                ensure!(quota >= 1_000, "cpu_quota must be at least 1000");
                ensure!(period > 0, "cpu_period must be greater than zero");
            }
            (Some(_), None) | (None, Some(_)) => {
                bail!("cpu_quota and cpu_period must be configured together");
            }
            (None, None) => {}
        }
        if let Some(pids) = self.container.resources.pids {
            ensure!(
                (1..=4_194_304).contains(&pids),
                "pids must be between 1 and 4194304"
            );
        }
        if require_rootfs && let Some(rootfs) = &self.container.rootfs {
            let init = resolve_container_path(rootfs, &self.container.init)?;
            ensure!(
                init.exists(),
                "init does not exist in rootfs: {}",
                init.display()
            );
        }
        for mount in &self.container.mounts {
            ensure!(
                mount.source.exists(),
                "bind source does not exist: {}",
                mount.source.display()
            );
            ensure!(
                safe_container_path(&mount.target),
                "unsafe bind target: {}",
                mount.target.display()
            );
        }
        for key in self.container.environment.keys() {
            ensure!(
                valid_env_key(key),
                "invalid environment variable name: {key}"
            );
        }
        for value in self.container.environment.values() {
            ensure!(
                !value.contains('\0'),
                "environment values cannot contain NUL bytes"
            );
        }
        if let Some(path) = &self.container.environment_file {
            ensure!(
                path.is_file(),
                "environment file does not exist: {}",
                path.display()
            );
            let source = fs::read_to_string(path)
                .with_context(|| format!("failed to read environment file {}", path.display()))?;
            let from_file = parse_environment(&source)
                .with_context(|| format!("failed to parse environment file {}", path.display()))?;
            let configured = std::mem::take(&mut self.container.environment);
            self.container.environment = from_file;
            self.container.environment.extend(configured);
        }
        Ok(())
    }
}

impl BinfmtConfig {
    /// Validates the target and interpreter without reading host runtime state.
    ///
    /// # Errors
    /// Returns an error for a native target or an invalid interpreter path/file.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.arch.is_native(),
            "binfmt target must differ from the host architecture"
        );
        let path = self
            .interpreter
            .to_str()
            .context("binfmt interpreter path must be UTF-8")?;
        ensure!(
            self.interpreter.is_absolute()
                && !path
                    .chars()
                    .any(|ch| ch.is_control() || matches!(ch, ':' | '\\')),
            "binfmt interpreter must be an absolute host path without control characters, ':' or '\\'"
        );
        let metadata =
            fs::metadata(&self.interpreter).context("failed to inspect binfmt interpreter")?;
        ensure!(
            metadata.is_file(),
            "binfmt interpreter must be a regular file"
        );
        ensure!(
            metadata.permissions().mode() & 0o111 != 0,
            "binfmt interpreter must be executable"
        );
        Ok(())
    }
}

impl Config {
    /// Validates and normalizes the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when a configuration value is invalid or cannot be
    /// normalized.
    pub fn validate(&mut self) -> Result<()> {
        <Self as Validate>::validate(self)
    }
}

impl Validate for Config {
    type Error = ConfigError;

    fn validate(&mut self) -> Result<()> {
        self.validate_inner(true)
    }
}

/// Resolves a path using rootfs-relative absolute symlink semantics.
///
/// # Errors
/// Returns errors for paths escaping rootfs, missing components, or symlink loops.
pub fn resolve_container_path(rootfs: &Path, path: &Path) -> Result<PathBuf> {
    let mut pending = path
        .components()
        .map(|part| part.as_os_str().to_owned())
        .collect::<VecDeque<_>>();
    let mut relative = PathBuf::new();
    let mut links = 0;
    while let Some(part) = pending.pop_front() {
        if part == "/" {
            relative.clear();
            continue;
        }
        if part == "." {
            continue;
        }
        if part == ".." {
            ensure!(relative.pop(), "container path escapes rootfs");
            continue;
        }
        relative.push(part);
        let candidate = rootfs.join(&relative);
        if fs::symlink_metadata(&candidate)?.file_type().is_symlink() {
            links += 1;
            ensure!(links <= 40, "too many symbolic links in container path");
            let target = fs::read_link(candidate)?;
            relative.pop();
            for component in target.components().rev() {
                pending.push_front(component.as_os_str().to_owned());
            }
        }
    }
    Ok(rootfs.join(relative))
}

fn absolute_from(base: &Path, path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    joined
        .canonicalize()
        .with_context(|| format!("failed to resolve path {}", joined.display()))
}

fn absolute_install_target(base: &Path, path: &Path) -> Result<PathBuf> {
    ensure!(
        !path
            .components()
            .any(|component| matches!(component, Component::ParentDir)),
        "install target cannot contain '..': {}",
        path.display()
    );
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let name = joined
        .file_name()
        .context("install target must name a rootfs directory or image")?;
    let parent = joined
        .parent()
        .context("install target has no parent directory")?
        .canonicalize()
        .with_context(|| format!("failed to resolve install parent for {}", joined.display()))?;
    Ok(parent.join(name))
}

pub(crate) fn safe_container_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
        && path != Path::new("/")
        && !path.starts_with("/.old_root")
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Tcp => "tcp",
        Protocol::Udp => "udp",
    }
}

fn valid_interface_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 16
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(crate) fn lock_document(path: &Path) -> Result<File> {
    let lock_path = path.with_file_name(format!(
        ".{}.lock",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&lock_path)
        .with_context(|| format!("failed to open config lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .context("failed to lock persistent config")?;
    Ok(lock)
}

pub(crate) fn write_document(path: &Path, document: &[serde_json::Value]) -> Result<()> {
    let encoded = serde_json::to_vec_pretty(document).map_err(|source| ConfigError::Context {
        context: "failed to serialize persistent JSON config".to_owned(),
        source: Box::new(source),
    })?;
    let metadata = match fs::metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary)?;
        if let Some(metadata) = metadata {
            chown(&temporary, Some(metadata.uid()), Some(metadata.gid()))?;
            fs::set_permissions(&temporary, metadata.permissions())?;
        }
        file.write_all(&encoded)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
            .with_context(|| format!("failed to commit persistent config {}", path.display()))?;
        File::open(path.parent().context("config path has no parent")?)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        fs::remove_file(&temporary).ok();
    }
    result
}
