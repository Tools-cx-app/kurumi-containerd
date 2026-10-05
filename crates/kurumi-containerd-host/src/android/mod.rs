use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use crate::{
    Result,
    error::{ErrorContext as _, ensure},
};
use kurumi_containerd_config::AndroidConfig;
use kurumi_containerd_helper::fs::{MountFlags, mount};

pub use crate::selinux::SelinuxGuard;

const GPU_DIRECTORIES: &[(&str, &str)] = &[
    ("/dev/dri", "renderD"),
    ("/dev", "kgsl"),
    ("/dev", "mali"),
    ("/dev", "video"),
];
const BINDER_DEVICES: &[&str] = &["/dev/binder", "/dev/hwbinder", "/dev/vndbinder"];

/// Mounts Android integration resources needed before switching roots.
///
/// # Errors
/// Returns errors when preparing mountpoints or binding storage fails.
pub fn setup_before_pivot(rootfs: &Path, config: &AndroidConfig) -> Result<()> {
    if !requested(config) {
        return Ok(());
    }
    tracing::debug!(rootfs = %rootfs.display(), "setting up Android integration before pivot");

    if config.storage {
        bind_path(
            Path::new("/storage/emulated/0"),
            &rootfs.join("storage/emulated/0"),
            true,
        )?;
    }
    Ok(())
}

/// Exposes configured Android devices and sockets after switching roots.
///
/// # Errors
/// Returns device, socket, or bind-mount setup errors.
pub fn setup_after_pivot(config: &AndroidConfig) -> Result<()> {
    if !requested(config) {
        return Ok(());
    }
    tracing::debug!("setting up Android integration after pivot");
    if config.gpu {
        for (directory, prefix) in GPU_DIRECTORIES {
            mirror_matching(directory, prefix)?;
        }
    }
    if config.binder {
        for source in BINDER_DEVICES {
            mirror_device(source)?;
        }
    }
    if config.termux_x11 {
        bind_socket(
            "/.old_root/data/data/com.termux/files/usr/tmp/.X11-unix/X5",
            "/tmp/.X11-unix/X5",
        )?;
    }
    if config.virgl {
        bind_socket(
            "/.old_root/data/data/com.termux/files/usr/tmp/.virgl_test",
            "/tmp/.virgl_test",
        )?;
    }
    if config.pulse_audio {
        bind_socket(
            "/.old_root/data/data/com.termux/files/usr/tmp/.pulse-socket",
            "/tmp/.pulse-socket",
        )?;
    }
    Ok(())
}

fn requested(config: &AndroidConfig) -> bool {
    config.storage
        || config.gpu
        || config.binder
        || config.termux_x11
        || config.virgl
        || config.pulse_audio
        || config.selinux_permissive
}

fn mirror_matching(directory: &str, prefix: &str) -> Result<()> {
    let host_directory = PathBuf::from("/.old_root").join(directory.trim_start_matches('/'));
    let Ok(entries) = fs::read_dir(&host_directory) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            let target = PathBuf::from(directory).join(entry.file_name());
            bind_path(&entry.path(), &target, false)?;
        }
    }
    Ok(())
}

fn mirror_device(source: &str) -> Result<()> {
    let source = PathBuf::from("/.old_root").join(source.trim_start_matches('/'));
    if !source.exists() {
        return Ok(());
    }
    let target = PathBuf::from("/").join(source.strip_prefix("/.old_root")?);
    bind_path(&source, &target, false)
}

fn bind_socket(source: &str, target: &str) -> Result<()> {
    let source = Path::new(source);
    ensure!(
        source.exists(),
        "Android integration socket is unavailable: {}",
        source.display()
    );
    bind_path(source, Path::new(target), false)
}

fn bind_path(source: &Path, target: &Path, recursive: bool) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(target)?;
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        File::create(target)?;
    }
    let flags = if recursive {
        MountFlags::BIND | MountFlags::REC
    } else {
        MountFlags::BIND
    };
    mount(Some(source), target, None, flags, None).with_context(|| {
        format!(
            "failed to bind {} to {}",
            source.display(),
            target.display()
        )
    })
}
