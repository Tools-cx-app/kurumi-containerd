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
    for line in info.display_lines() {
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

#[cfg(test)]
#[path = "../tests/unit/output/tests.rs"]
mod tests;
