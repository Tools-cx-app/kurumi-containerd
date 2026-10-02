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
mod tests {
    use super::*;

    #[test]
    fn pid_is_numeric_and_foreground_reports_completion() {
        let mut out = Vec::new();
        pid(&mut out, 1234).unwrap();
        assert_eq!(out, b"1234\n");
        out.clear();
        started(&mut out, "demo", 1234, true).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("demo"));
        assert!(text.contains("finished"));
        assert!(!text.contains("started"));
    }

    #[test]
    fn empty_results_are_explicit_and_writes_are_fallible() {
        let mut out = Vec::new();
        containers(&mut out, &[]).unwrap();
        recovered(&mut out, &[]).unwrap();
        check(&mut out, "Namespaces", false, "permission denied").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Running containers: 0"));
        assert!(text.contains("No containers required recovery"));
        assert!(text.contains("unavailable (permission denied)"));
        assert!(!text.contains("INFO"));
        let mut full = &mut [][..];
        let error = pid(&mut full, 1234).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
    }

    #[test]
    fn only_output_broken_pipes_are_ignored() {
        let error = write(|_| Err(io::ErrorKind::BrokenPipe.into())).unwrap_err();
        assert!(error.is_broken_pipe());
        let error = write(|_| Err(io::ErrorKind::PermissionDenied.into())).unwrap_err();
        assert!(!error.is_broken_pipe());
    }
}
