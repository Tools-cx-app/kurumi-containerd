use crate::registry::Entry;
use anyhow::{Result, bail};
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    Install {
        archive: PathBuf,
        size: Option<String>,
        force: bool,
    },
    Start(bool),
    Stop,
    Restart(bool),
    Enter(String),
    Run(Vec<String>),
    Info,
    Pid,
    Show,
    Scan,
    Check,
}
impl Action {
    pub(super) fn args(&self, name: &str) -> Vec<OsString> {
        let mut a = if matches!(self, Self::Check) {
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
            Self::Enter(v) => a.extend(["enter".into(), v.into()]),
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
            Self::Enter(_) | Self::Run(_) | Self::Start(true) | Self::Restart(true)
        ) || foreground && matches!(self, Self::Start(_) | Self::Restart(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionKind {
    Install,
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
pub(super) const ACTIONS: [ActionKind; 13] = [
    ActionKind::Install,
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
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Install => "Install",
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
            Self::Enter => &["User (default root)"],
            Self::Run => &["Executable", "Argument 1 (Tab to add another)"],
            _ => &[],
        }
    }
    pub(super) fn confirm(self) -> bool {
        matches!(
            self,
            Self::Stop | Self::Restart | Self::ForegroundRestart | Self::Scan
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
            Self::Start => Action::Start(false),
            Self::ForegroundStart => Action::Start(true),
            Self::Stop => Action::Stop,
            Self::Restart => Action::Restart(false),
            Self::ForegroundRestart => Action::Restart(true),
            Self::Enter => Action::Enter(if f[0].trim().is_empty() {
                "root".into()
            } else {
                f[0].trim().into()
            }),
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
    if k == ActionKind::Check {
        return Ok(());
    }
    if k == ActionKind::Install && e.install_ready && e.info.is_err() {
        return Ok(());
    }
    let active = e
        .info
        .as_ref()
        .map_err(|x| anyhow::anyhow!("{x:#}"))?
        .active;
    if matches!(
        k,
        ActionKind::Start | ActionKind::ForegroundStart | ActionKind::Install
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
mod tests;
