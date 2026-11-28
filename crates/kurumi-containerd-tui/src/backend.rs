// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
    process::{Child, Command, Stdio},
};

use kurumi_containerd_config::ConfigEntry;
use kurumi_containerd_daemon::{Client, Operation, Request, Response};
use kurumi_containerd_error::{Result, RuntimeError, bail};
use kurumi_containerd_runtime::ContainerInfo;

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

    pub(super) fn load_entries(&self) -> Result<Vec<Entry>> {
        Ok(ConfigEntry::load_home()?
            .into_iter()
            .map(|config| self.load_entry(config))
            .collect())
    }

    pub(super) fn refresh(&self, entries: &mut [Entry]) {
        for entry in entries {
            self.refresh_entry(entry);
        }
    }

    #[allow(clippy::unused_self)]
    pub(super) fn can_open_terminal(&self) -> bool {
        terminal_command().as_ref().is_ok_and(Option::is_some)
    }

    pub(super) fn launch(&self, action: &Action, selected: Option<&Entry>) -> Result<Launch> {
        let args = action.args(selected.map_or("", |entry| entry.config.name.as_str()));
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

    fn load_entry(&self, config: ConfigEntry) -> Entry {
        let mut entry = Entry {
            config,
            info: Err(RuntimeError::Message("status unavailable".to_owned())),
            foreground: false,
            install_ready: false,
        };
        self.refresh_entry(&mut entry);
        entry
    }

    fn refresh_entry(&self, entry: &mut Entry) {
        entry.install_ready = entry.config.load_for_install().is_ok();
        entry.info = self.info(entry);
    }

    fn info(&self, entry: &mut Entry) -> Result<ContainerInfo> {
        let config = entry.config.load_config()?;
        entry.foreground = config.container.foreground;
        match Client::new(self.exe, false).request(&Request {
            config: Some(config),
            operation: Operation::Info,
        })? {
            Response::Info(info) => Ok(info),
            response => bail!("unexpected daemon response: {response:?}"),
        }
    }
}

#[cfg(test)]
#[path = "backend/tests.rs"]
mod tests;
