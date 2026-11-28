// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{ffi::OsString, path::PathBuf};

use kurumi_containerd_error::{Result, RuntimeError, bail};

use crate::registry::Entry;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    Install {
        archive: PathBuf,
        size: Option<String>,
        force: bool,
    },
    Backup {
        output: PathBuf,
    },
    Export {
        output: PathBuf,
    },
    Inspect {
        package: PathBuf,
    },
    Import {
        package: PathBuf,
        name: Option<String>,
        size: Option<String>,
        mappings: Vec<String>,
    },
    Start(bool),
    Stop,
    Restart(bool),
    Enter {
        user: String,
        shell: Option<String>,
    },
    Run(Vec<String>),
    Info,
    Pid,
    Show,
    Scan,
    Check,
}
impl Action {
    pub(super) fn args(&self, name: &str) -> Vec<OsString> {
        let mut a = if matches!(
            self,
            Self::Check
                | Self::Backup { .. }
                | Self::Export { .. }
                | Self::Import { .. }
                | Self::Inspect { .. }
        ) {
            vec![]
        } else {
            vec!["--name".into(), name.into()]
        };
        match self {
            Self::Install {
                archive,
                size,
                force,
            } => {
                a.extend(["install".into(), archive.as_os_str().to_owned()]);
                if let Some(v) = size {
                    a.extend(["--size".into(), v.into()]);
                }
                if *force {
                    a.push("--force".into());
                }
            }
            Self::Backup { output } => {
                a.extend(["backup".into(), name.into(), output.as_os_str().to_owned()]);
            }
            Self::Export { output } => {
                a.extend(["export".into(), name.into(), output.as_os_str().to_owned()]);
            }
            Self::Inspect { package } => {
                a.extend(["import".into(), package.into(), "--inspect".into()]);
            }
            Self::Import {
                package,
                name,
                size,
                mappings,
            } => {
                if let Some(name) = name {
                    a.extend(["--name".into(), name.into()]);
                }
                a.extend([OsString::from("import"), package.into()]);
                if let Some(size) = size {
                    a.extend(["--size".into(), size.into()]);
                }
                for mapping in mappings {
                    a.extend(["--map".into(), mapping.into()]);
                }
            }
            Self::Start(f) | Self::Restart(f) => {
                a.push(if matches!(self, Self::Start(_)) {
                    "start".into()
                } else {
                    "restart".into()
                });
                if *f {
                    a.push("--foreground".into());
                }
            }
            Self::Stop => a.push("stop".into()),
            Self::Enter { user, shell } => {
                a.extend(["enter".into(), user.into()]);
                if let Some(shell) = shell {
                    a.extend(["--shell".into(), shell.into()]);
                }
            }
            Self::Run(v) => {
                a.push("run".into());
                a.extend(v.iter().map(OsString::from));
            }
            Self::Info => a.push("info".into()),
            Self::Pid => a.push("pid".into()),
            Self::Show => a.push("show".into()),
            Self::Scan => a.push("scan".into()),
            Self::Check => a.push("check".into()),
        }
        a
    }
    pub(super) fn needs_terminal(&self, foreground: bool) -> bool {
        matches!(
            self,
            Self::Enter { .. } | Self::Run(_) | Self::Start(true) | Self::Restart(true)
        ) || foreground && matches!(self, Self::Start(_) | Self::Restart(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionKind {
    Install,
    Backup,
    Export,
    Inspect,
    Import,
    Start,
    ForegroundStart,
    Stop,
    Restart,
    ForegroundRestart,
    Enter,
    Run,
    Info,
    Pid,
    Show,
    Scan,
    Check,
}
pub(super) const ACTIONS: [ActionKind; 17] = [
    ActionKind::Install,
    ActionKind::Backup,
    ActionKind::Export,
    ActionKind::Inspect,
    ActionKind::Import,
    ActionKind::Start,
    ActionKind::ForegroundStart,
    ActionKind::Stop,
    ActionKind::Restart,
    ActionKind::ForegroundRestart,
    ActionKind::Enter,
    ActionKind::Run,
    ActionKind::Info,
    ActionKind::Pid,
    ActionKind::Show,
    ActionKind::Scan,
    ActionKind::Check,
];
impl ActionKind {
    pub(super) fn needs_entry(self) -> bool {
        !matches!(self, Self::Check | Self::Inspect | Self::Import)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Install => "Install",
            Self::Backup => "Backup",
            Self::Export => "Export package",
            Self::Inspect => "Inspect package",
            Self::Import => "Import package",
            Self::Start => "Start",
            Self::ForegroundStart => "Start foreground",
            Self::Stop => "Stop",
            Self::Restart => "Restart",
            Self::ForegroundRestart => "Restart foreground",
            Self::Enter => "Enter",
            Self::Run => "Run",
            Self::Info => "Info",
            Self::Pid => "PID",
            Self::Show => "Show",
            Self::Scan => "Scan",
            Self::Check => "Check host",
        }
    }
    pub(super) fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Install => &["Archive path", "Image size (optional)"],
            Self::Backup => &["Output path (.zst/.xz/.zip/.tar)"],
            Self::Export => &["New package path (.kurumi)"],
            Self::Inspect => &["Package path"],
            Self::Import => &[
                "Package path",
                "New name (default: package name)",
                "Image size (blank for directory)",
                "Mapping: KEY=/host/path (Tab to add another)",
            ],
            Self::Enter => &["User (default root)", "Shell command (optional)"],
            Self::Run => &["Executable", "Argument 1 (Tab to add another)"],
            _ => &[],
        }
    }
    pub(super) fn confirm(self) -> bool {
        matches!(
            self,
            Self::Stop | Self::Restart | Self::ForegroundRestart | Self::Scan | Self::Import
        )
    }
    pub(super) fn action(self, f: &[String], force: bool) -> Result<Action> {
        Ok(match self {
            Self::Install => {
                if f[0].trim().is_empty() {
                    bail!("archive path is required")
                }
                Action::Install {
                    archive: f[0].trim().into(),
                    size: (!f[1].trim().is_empty()).then(|| f[1].trim().into()),
                    force,
                }
            }
            Self::Backup | Self::Export => {
                if f[0].trim().is_empty() {
                    bail!("output path is required")
                }
                let output = f[0].trim().into();
                if self == Self::Export {
                    Action::Export { output }
                } else {
                    Action::Backup { output }
                }
            }
            Self::Inspect | Self::Import => {
                let package = f[0].trim();
                if package.is_empty() {
                    bail!("package path is required");
                }
                if self == Self::Inspect {
                    Action::Inspect {
                        package: package.into(),
                    }
                } else {
                    let mappings: Vec<_> = f[3..]
                        .iter()
                        .map(|value| value.trim())
                        .filter(|value| !value.is_empty())
                        .collect();
                    if mappings.iter().any(|value| {
                        !value
                            .split_once('=')
                            .is_some_and(|(key, path)| !key.is_empty() && path.starts_with('/'))
                    }) {
                        bail!("expected mapping KEY=/absolute/host/path");
                    }
                    Action::Import {
                        package: package.into(),
                        name: (!f[1].trim().is_empty()).then(|| f[1].trim().into()),
                        size: (!f[2].trim().is_empty()).then(|| f[2].trim().into()),
                        mappings: mappings.into_iter().map(str::to_owned).collect(),
                    }
                }
            }
            Self::Start => Action::Start(false),
            Self::ForegroundStart => Action::Start(true),
            Self::Stop => Action::Stop,
            Self::Restart => Action::Restart(false),
            Self::ForegroundRestart => Action::Restart(true),
            Self::Enter => {
                let shell = f[1].trim();
                if !shell.is_empty()
                    && !shlex::split(shell)
                        .and_then(|words| words.into_iter().next())
                        .is_some_and(|program| program.starts_with('/'))
                {
                    bail!("login shell requires an absolute container path and valid quoting")
                }
                Action::Enter {
                    user: if f[0].trim().is_empty() {
                        "root".into()
                    } else {
                        f[0].trim().into()
                    },
                    shell: (!shell.is_empty()).then(|| shell.into()),
                }
            }
            Self::Run => {
                if f[0].trim().is_empty() {
                    bail!("executable is required")
                }
                Action::Run(
                    std::iter::once(f[0].trim().into())
                        .chain(f[1..].iter().filter(|v| !v.is_empty()).cloned())
                        .collect(),
                )
            }
            Self::Info => Action::Info,
            Self::Pid => Action::Pid,
            Self::Show => Action::Show,
            Self::Scan => Action::Scan,
            Self::Check => Action::Check,
        })
    }
}
pub(super) fn allowed(k: ActionKind, e: &Entry, terminal: bool) -> Result<()> {
    if !k.needs_entry() {
        return Ok(());
    }
    if k == ActionKind::Install && e.install_ready && e.info.is_err() {
        return Ok(());
    }
    let active = e
        .info
        .as_ref()
        .map_err(|x| RuntimeError::Message(format!("{x:#}")))?
        .active;
    if matches!(
        k,
        ActionKind::Start
            | ActionKind::ForegroundStart
            | ActionKind::Install
            | ActionKind::Backup
            | ActionKind::Export
    ) && active
    {
        bail!("stop the container first")
    }
    if matches!(
        k,
        ActionKind::Stop | ActionKind::Enter | ActionKind::Run | ActionKind::Pid
    ) && !active
    {
        bail!("container is not running")
    }
    if (matches!(
        k,
        ActionKind::Enter
            | ActionKind::Run
            | ActionKind::ForegroundStart
            | ActionKind::ForegroundRestart
    ) || e.foreground && matches!(k, ActionKind::Start | ActionKind::Restart))
        && !terminal
    {
        bail!("requires a Linux graphical session and xterm, kitty or alacritty")
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/action/tests.rs"]
mod tests;
