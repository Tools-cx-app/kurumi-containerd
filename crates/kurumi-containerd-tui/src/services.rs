// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::collections::BTreeMap;
use std::process::ExitStatus;

use kurumi_containerd_error::{ErrorContext as _, Result, bail, ensure};

use crate::backend::{Backend, CommandTask};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Manager {
    Systemd,
    OpenRc,
}

impl Manager {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::OpenRc => "OpenRC",
        }
    }
}

pub(super) struct Service {
    pub(super) name: String,
    pub(super) state: String,
    pub(super) startup: String,
}

fn inventory(body: &str) -> Result<(Manager, Vec<Service>)> {
    let incomplete = "Incomplete service list (capture limit: 1 MiB). Refresh to retry.";
    let (body, _) = body.rsplit_once("[end services]\n").context(incomplete)?;
    let mut entries = BTreeMap::new();
    if let Some((_, body)) = body.split_once("[systemd files]\n") {
        let (files, units) = body.split_once("[systemd units]\n").context(incomplete)?;
        for (line, is_unit) in files
            .lines()
            .map(|line| (line, false))
            .chain(units.lines().map(|line| (line, true)))
        {
            let mut fields = line.split_whitespace();
            let Some(name) = fields
                .next()
                .filter(|name| name.ends_with(".service") && valid_name(name))
            else {
                continue;
            };
            let Some(value) = fields.nth(usize::from(is_unit)) else {
                continue;
            };
            let service = entries.entry(name).or_insert_with(|| Service {
                name: name.into(),
                state: "inactive".into(),
                startup: "—".into(),
            });
            if is_unit {
                value.clone_into(&mut service.state);
            } else {
                value.clone_into(&mut service.startup);
            }
        }
        return Ok((Manager::Systemd, entries.into_values().collect()));
    }

    let (_, body) = body.split_once("[openrc services]\n").context(incomplete)?;
    let (services, body) = body.split_once("[openrc status]\n").context(incomplete)?;
    let (status, default) = body.split_once("[openrc default]\n").context(incomplete)?;
    for name in services
        .lines()
        .map(str::trim)
        .filter(|name| valid_name(name))
    {
        entries.insert(
            name,
            Service {
                name: name.into(),
                state: "stopped".into(),
                startup: "—".into(),
            },
        );
    }
    for line in status.lines() {
        if let Some((name, state)) = line.split_once('[')
            && let Some(state) = state.trim_end().strip_suffix(']')
            && let Some(service) = entries.get_mut(name.trim())
        {
            state.trim().clone_into(&mut service.state);
        }
    }
    for line in default.lines() {
        if let Some((name, levels)) = line.split_once('|')
            && levels.split_whitespace().any(|level| level == "default")
            && let Some(service) = entries.get_mut(name.trim())
        {
            "default".clone_into(&mut service.startup);
        }
    }
    Ok((Manager::OpenRc, entries.into_values().collect()))
}

/// Service tools interpret leading options and systemd unit glob patterns even without a shell.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains(['/', '*', '?', '[', ']'])
        && !name.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Operation {
    Status,
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
    Journal,
}

impl Operation {
    pub(super) fn verb(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Journal => "journal",
        }
    }

    pub(super) fn changes_service(self) -> bool {
        !matches!(self, Self::Status | Self::Journal)
    }

    fn command(self, manager: Manager, name: &str) -> Result<Vec<String>> {
        let verb = self.verb();
        let args = match (manager, self) {
            (Manager::Systemd, Self::Journal) => {
                vec!["journalctl", "--no-pager", "-n", "100", "-u", name]
            }
            (Manager::Systemd, _) => vec![
                "systemctl",
                "--system",
                "--no-pager",
                "--no-ask-password",
                verb,
                "--",
                name,
            ],
            (Manager::OpenRc, Self::Journal) => bail!("Journal is available for systemd services"),
            (Manager::OpenRc, Self::Enable) => vec!["rc-update", "add", name, "default"],
            (Manager::OpenRc, Self::Disable) => vec!["rc-update", "del", name, "default"],
            (Manager::OpenRc, _) => vec!["rc-service", name, verb],
        };
        Ok(args.into_iter().map(String::from).collect())
    }
}

enum Pending {
    Inventory,
    Service(Operation, String),
}

pub(super) struct Services {
    pub(super) container: String,
    pub(super) manager: Option<Manager>,
    pub(super) rows: Vec<Service>,
    pub(super) selected: usize,
    pub(super) output: String,
    pub(super) scroll: u16,
    pub(super) confirm: Option<Operation>,
    task: Option<(CommandTask, Pending)>,
}

impl Services {
    pub(super) fn open(container: &str, backend: &Backend<'_>) -> Self {
        let mut services = Self {
            container: container.into(),
            manager: None,
            rows: Vec::new(),
            selected: 0,
            output: String::new(),
            scroll: 0,
            confirm: None,
            task: None,
        };
        services.refresh(backend);
        services
    }

    pub(super) fn busy(&self) -> bool {
        self.task.is_some()
    }

    pub(super) fn refresh(&mut self, backend: &Backend<'_>) {
        let command = ["/bin/sh", "-c", include_str!("services.sh")]
            .map(String::from)
            .into();
        self.launch(backend, command, Pending::Inventory);
    }

    fn launch(&mut self, backend: &Backend<'_>, command: Vec<String>, pending: Pending) {
        self.scroll = 0;
        match backend.run_in_container(&self.container, command) {
            Ok(task) => self.task = Some((task, pending)),
            Err(error) => {
                self.output = format!("Could not run container command: {error:#}");
                self.rows.clear();
            }
        }
    }

    pub(super) fn request(&mut self, operation: Operation, backend: &Backend<'_>) -> Result<()> {
        let (Some(manager), Some(service)) = (self.manager, self.rows.get(self.selected)) else {
            return Ok(());
        };
        ensure!(!self.busy(), "A service operation is running");
        if operation.changes_service() && self.confirm != Some(operation) {
            self.confirm = Some(operation);
            return Ok(());
        }
        self.confirm = None;
        let command = operation.command(manager, &service.name)?;
        self.launch(
            backend,
            command,
            Pending::Service(operation, service.name.clone()),
        );
        Ok(())
    }

    pub(super) fn poll(&mut self, backend: &Backend<'_>) {
        let Some((task, _)) = &mut self.task else {
            return;
        };
        let result = match task.poll_result() {
            Ok(Some(result)) => result,
            Ok(None) => return,
            Err(error) => {
                self.task = None;
                self.rows.clear();
                self.output = format!("{error:#}");
                return;
            }
        };
        if let Some((_, pending)) = self.task.take() {
            self.finish(pending, result, backend);
        }
    }

    fn finish(
        &mut self,
        pending: Pending,
        (status, body): (ExitStatus, String),
        backend: &Backend<'_>,
    ) {
        match pending {
            Pending::Inventory => {
                let result = if status.success() {
                    inventory(&body)
                } else {
                    Err(kurumi_containerd_error::RuntimeError::Message(format!(
                        "Service list ({status})\n{body}"
                    )))
                };
                match result {
                    Ok((manager, rows)) => {
                        let previous = self.rows.get(self.selected);
                        self.selected = previous
                            .and_then(|old| rows.iter().position(|s| s.name == old.name))
                            .unwrap_or(0);
                        self.manager = Some(manager);
                        self.rows = rows;
                    }
                    Err(error) => {
                        self.rows.clear();
                        self.manager = None;
                        self.output = format!("{error:#}");
                    }
                }
            }
            Pending::Service(operation, name) => {
                self.output = format!("{} {name} ({status})\n{body}", operation.verb());
                if operation.changes_service() {
                    self.refresh(backend);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/services/tests.rs"]
mod tests;
