use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{Result, error::ErrorContext as _};
use procfs::process::all_processes;
use serde::Serialize;
use uuid::Uuid;

use super::{ContainerState, process_namespace_inode};
use crate::{Runtime, container::init::InitSystem};

#[derive(Debug, Clone, Serialize)]
pub struct ContainerInfo {
    pub name: String,
    pub active: bool,
    pub init_pid: Option<i32>,
    pub monitor_pid: Option<i32>,
    pub rootfs: PathBuf,
    pub uuid: Option<Uuid>,
    pub init_system: Option<InitSystem>,
    pub generation: Option<u64>,
    pub uptime_seconds: Option<u64>,
    pub memory_kb: Option<u64>,
    pub processes: Option<usize>,
}

impl std::fmt::Display for ContainerInfo {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.display_lines().join("\n"))
    }
}

impl ContainerInfo {
    #[must_use]
    pub fn display_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Name: {}", self.name)];
        if self.active {
            lines.extend([
                format!(
                    "Active: active (running) for {}",
                    format_duration(self.uptime_seconds.unwrap_or_default())
                ),
                format!(
                    "Main PID: {} ({})",
                    self.init_pid.unwrap_or_default(),
                    self.init_system.unwrap_or_default()
                ),
                format!("Monitor PID: {}", self.monitor_pid.unwrap_or_default()),
                format!("Tasks: {}", self.processes.unwrap_or_default()),
                format!(
                    "Memory: {}",
                    format_memory(self.memory_kb.unwrap_or_default())
                ),
                format!("Generation: {}", self.generation.unwrap_or_default()),
            ]);
        } else {
            lines.push("Active: inactive (dead)".to_owned());
        }
        lines.extend([
            format!("Rootfs: {}", self.rootfs.display()),
            format!(
                "UUID: {}",
                self.uuid
                    .map_or_else(|| "unassigned".to_owned(), |uuid| uuid.to_string())
            ),
        ]);
        lines
    }
}

impl Runtime {
    /// Returns the current container state and resource usage.
    ///
    /// # Errors
    ///
    /// Returns an error when state or procfs cannot be read.
    pub fn info(&self) -> Result<ContainerInfo> {
        let Some(state) = self.state()? else {
            let rootfs = self
                .config
                .container
                .rootfs
                .as_ref()
                .or(self.config.container.rootfs_image.as_ref())
                .context("container has no configured rootfs")?
                .clone();
            return Ok(ContainerInfo {
                name: self.config.container.name.clone(),
                active: false,
                init_pid: None,
                monitor_pid: None,
                rootfs,
                uuid: self.config.container.uuid,
                init_system: None,
                generation: None,
                uptime_seconds: None,
                memory_kb: None,
                processes: None,
            });
        };
        let (memory_kb, processes) = collect_usage(&state)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        Ok(ContainerInfo {
            name: state.name,
            active: true,
            init_pid: Some(state.init_pid),
            monitor_pid: Some(state.monitor_pid),
            rootfs: state.rootfs,
            uuid: Some(state.uuid),
            init_system: Some(state.init_system),
            generation: Some(state.generation),
            uptime_seconds: Some(now.saturating_sub(state.started_at_unix)),
            memory_kb: Some(memory_kb),
            processes: Some(processes),
        })
    }

    /// Returns the validated container init PID.
    ///
    /// # Errors
    ///
    /// Returns an error when the container is not running.
    pub fn pid(&self) -> Result<i32> {
        Ok(self.require_state()?.init_pid)
    }
}

fn collect_usage(state: &ContainerState) -> Result<(u64, usize)> {
    let mut memory_kb = 0;
    let mut processes = 0;
    for process in all_processes()? {
        let Ok(process) = process else {
            continue;
        };
        if process_namespace_inode(&process, "pid").ok() != Some(state.pid_namespace_inode) {
            continue;
        }
        processes += 1;
        if let Ok(status) = process.status() {
            memory_kb += status.vmrss.unwrap_or(0);
        }
    }
    Ok((memory_kb, processes))
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    if days > 0 {
        format!("{days}d {hours:02}h {minutes:02}m {seconds:02}s")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn format_memory(kibibytes: u64) -> String {
    if kibibytes >= 1024 * 1024 {
        format_unit(kibibytes, 1024 * 1024, "GiB")
    } else if kibibytes >= 1024 {
        format_unit(kibibytes, 1024, "MiB")
    } else {
        format!("{kibibytes} KiB")
    }
}

fn format_unit(value: u64, unit: u64, suffix: &str) -> String {
    let whole = value / unit;
    let decimal = value % unit * 10 / unit;
    format!("{whole}.{decimal} {suffix}")
}

#[cfg(test)]
mod tests;
