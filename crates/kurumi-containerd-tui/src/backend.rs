// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use anyhow::Result;
use kurumi_containerd_config::{Config, ConfigPointer};
use kurumi_containerd_runtime::{ContainerQuery, Runtime};
use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
    process::{Child, Command, Stdio},
};

use crate::{action::Action, registry::Entry, terminal::terminal_command};

pub(super) struct Backend<'a> {
    exe: &'a Path,
}

pub(super) enum Launch {
    Terminal,
    Captured(CommandTask),
    Failed(String),
}

pub(super) struct CommandTask {
    child: Child,
    output: File,
}

impl CommandTask {
    pub(super) fn poll(&mut self) -> Result<Option<String>> {
        let Some(status) = self.child.try_wait()? else {
            return Ok(None);
        };
        self.output.rewind()?;
        let mut bytes = Vec::new();
        self.output
            .by_ref()
            .take(1024 * 1024)
            .read_to_end(&mut bytes)?;
        Ok(Some(format!(
            "Exit: {status}\n{}",
            String::from_utf8_lossy(&bytes)
        )))
    }
}

impl<'a> Backend<'a> {
    pub(super) fn new(exe: &'a Path) -> Self {
        Self { exe }
    }

    #[allow(clippy::unused_self)]
    pub(super) fn load_entries(&self) -> Result<Vec<Entry>> {
        Ok(ConfigPointer::load_home()?
            .into_iter()
            .map(load_entry)
            .collect())
    }

    #[allow(clippy::unused_self)]
    pub(super) fn refresh(&self, entries: &mut [Entry]) {
        for entry in entries {
            refresh_entry(entry);
        }
    }

    #[allow(clippy::unused_self)]
    pub(super) fn can_open_terminal(&self) -> bool {
        terminal_command().as_ref().is_ok_and(Option::is_some)
    }

    pub(super) fn launch(&self, action: &Action, selected: Option<&Entry>) -> Result<Launch> {
        let args = action.args(selected.map_or("", |entry| entry.pointer.name.as_str()));
        if action.needs_terminal(selected.is_some_and(|entry| entry.foreground)) {
            let Ok(Some((path, program))) = terminal_command() else {
                return Ok(Launch::Failed("No graphical terminal available".into()));
            };
            let mut command = Command::new(path);
            match program {
                "xterm" => command.args(["-hold", "-e"]),
                "kitty" => command.args(["--hold", "--"]),
                _ => command.args(["--hold", "-e"]),
            };
            return Ok(
                match command
                    .arg(self.exe)
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(_) => Launch::Terminal,
                    Err(error) => Launch::Failed(format!("terminal launch failed: {error}")),
                },
            );
        }
        let output = tempfile::tempfile()?;
        let stderr = output.try_clone()?;
        let stdout = output.try_clone()?;
        Ok(
            match Command::new(self.exe)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr))
                .spawn()
            {
                Ok(child) => Launch::Captured(CommandTask { child, output }),
                Err(error) => Launch::Failed(format!("command launch failed: {error}")),
            },
        )
    }
}

fn load_entry(pointer: ConfigPointer) -> Entry {
    let install_ready = Config::load_for_install(&pointer.file).is_ok();
    let (info, foreground) = match Config::load(&pointer.file) {
        Ok(config) => {
            let foreground = config.container.foreground;
            (
                Runtime::new(config)
                    .and_then(|runtime| ContainerQuery::info(&runtime))
                    .map_err(Into::into),
                foreground,
            )
        }
        Err(error) => (Err(error.into()), false),
    };
    Entry {
        pointer,
        info,
        foreground,
        install_ready,
    }
}

fn refresh_entry(entry: &mut Entry) {
    entry.install_ready = Config::load_for_install(&entry.pointer.file).is_ok();
    match Config::load(&entry.pointer.file) {
        Ok(config) => {
            entry.foreground = config.container.foreground;
            entry.info = Runtime::new(config)
                .and_then(|runtime| ContainerQuery::info(&runtime))
                .map_err(Into::into);
        }
        Err(error) => entry.info = Err(error.into()),
    }
}

#[cfg(test)]
#[path = "backend/tests.rs"]
mod tests;
