// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fmt,
    io::{self, Write},
};

use kurumi_containerd_runtime::{ContainerInfo, ContainerState};

#[derive(Debug)]
pub struct OutputError(io::Error);

impl fmt::Display for OutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "failed to write command output: {}", self.0)
    }
}

impl std::error::Error for OutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl OutputError {
    pub fn is_broken_pipe(&self) -> bool {
        self.0.kind() == io::ErrorKind::BrokenPipe
    }
}

pub fn write(render: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> Result<(), OutputError> {
    render(&mut io::stdout().lock()).map_err(OutputError)
}

pub fn pid(out: &mut dyn Write, pid: i32) -> io::Result<()> {
    writeln!(out, "{pid}")
}

pub fn started(out: &mut dyn Write, name: &str, pid: i32, foreground: bool) -> io::Result<()> {
    if foreground {
        writeln!(out, "Container {name} finished")
    } else {
        writeln!(out, "Container {name} started (PID {pid})")
    }
}

pub fn info(out: &mut dyn Write, info: &ContainerInfo) -> io::Result<()> {
    for line in info_lines(info) {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

pub fn containers(out: &mut dyn Write, states: &[ContainerState]) -> io::Result<()> {
    for state in states {
        writeln!(
            out,
            "{}  PID {}  {}",
            state.name,
            state.init_pid,
            state.rootfs.display()
        )?;
    }
    writeln!(out, "Running containers: {}", states.len())
}

pub fn recovered(out: &mut dyn Write, states: &[ContainerState]) -> io::Result<()> {
    if states.is_empty() {
        return writeln!(out, "No containers required recovery");
    }
    for state in states {
        writeln!(out, "Recovered {} (PID {})", state.name, state.init_pid)?;
    }
    writeln!(out, "Recovered containers: {}", states.len())
}

pub fn check(
    out: &mut dyn Write,
    capability: &str,
    available: bool,
    detail: &str,
) -> io::Result<()> {
    let status = if available {
        "available"
    } else {
        "unavailable"
    };
    writeln!(out, "{capability}: {status} ({detail})")
}

fn info_lines(info: &ContainerInfo) -> Vec<String> {
    let mut lines = vec![format!("Name: {}", info.name)];
    if info.active {
        lines.extend([
            format!(
                "Active: active (running) for {}",
                format_duration(info.uptime_seconds.unwrap_or_default())
            ),
            format!(
                "Main PID: {} ({})",
                info.init_pid.unwrap_or_default(),
                info.init_system.unwrap_or_default()
            ),
            format!("Monitor PID: {}", info.monitor_pid.unwrap_or_default()),
            format!("Tasks: {}", info.processes.unwrap_or_default()),
            format!(
                "Memory: {}",
                format_memory(info.memory_kb.unwrap_or_default())
            ),
            format!("Generation: {}", info.generation.unwrap_or_default()),
        ]);
    } else {
        lines.push("Active: inactive (dead)".to_owned());
    }
    lines.extend([
        format!("Rootfs: {}", info.rootfs.display()),
        format!(
            "UUID: {}",
            info.uuid
                .map_or_else(|| "unassigned".to_owned(), |uuid| uuid.to_string())
        ),
    ]);
    lines
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
#[path = "../tests/unit/output/tests.rs"]
mod tests;
