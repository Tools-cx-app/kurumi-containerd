// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::path::PathBuf;

use kurumi_containerd_config::Config;
use kurumi_containerd_error::{ErrorContext as _, Result, ensure};
use kurumi_containerd_runtime::{ContainerInfo, ContainerState, Runtime, RuntimeRegistry};
use serde::{Deserialize, Serialize};

/// A transport-independent request with the caller's resolved configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub config: Option<Config>,
    pub operation: Operation,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Operation {
    Install {
        archive: PathBuf,
        size: Option<u64>,
        force: bool,
    },
    Backup {
        output: PathBuf,
    },
    Start {
        foreground: bool,
    },
    Stop,
    Restart {
        foreground: bool,
    },
    Enter {
        user: String,
        shell: Option<String>,
    },
    Run {
        command: Vec<String>,
    },
    Info,
    Pid,
    List,
    Scan,
}

impl Operation {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Install { .. } => "install",
            Self::Backup { .. } => "backup",
            Self::Start { .. } => "start",
            Self::Stop => "stop",
            Self::Restart { .. } => "restart",
            Self::Enter { .. } => "enter",
            Self::Run { .. } => "run",
            Self::Info => "info",
            Self::Pid => "pid",
            Self::List => "list",
            Self::Scan => "scan",
        }
    }

    pub(crate) fn uses_stdio(&self) -> bool {
        matches!(
            self,
            Self::Start { .. } | Self::Restart { .. } | Self::Run { .. } | Self::Enter { .. }
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
pub enum Response {
    Done,
    Started {
        state: ContainerState,
        foreground: bool,
    },
    Status(i32),
    Info(ContainerInfo),
    Pid(i32),
    Containers(Vec<ContainerState>),
    Error(String),
}

#[derive(Default)]
pub struct Service {
    registry: RuntimeRegistry,
}

pub struct Job {
    runtime: Runtime,
    operation: Operation,
    configured_foreground: bool,
}

impl Service {
    /// Resolves the request against the daemon's state before handing it to a worker.
    /// # Errors
    /// Returns an error when configuration or in-memory state is unavailable.
    pub fn prepare(&mut self, request: Request) -> Result<Result<Job, Response>> {
        if matches!(request.operation, Operation::List | Operation::Scan) {
            return Ok(Err(Response::Containers(self.registry.list()?)));
        }
        let config = request
            .config
            .context("container configuration is required")?;
        let name = &config.container.name;
        ensure!(
            !name.is_empty()
                && name.len() <= 255
                && name != "."
                && name != ".."
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
            "invalid container name: {name}"
        );
        let configured_foreground = config.container.foreground;
        Ok(Ok(Job {
            runtime: self.registry.open(config)?,
            operation: request.operation,
            configured_foreground,
        }))
    }
}

impl Job {
    /// Executes a prepared operation in a dedicated single-threaded process.
    /// Frontends should submit requests through `Client` when running on a threaded executor.
    /// # Errors
    /// Returns container operation failures.
    pub fn execute(self) -> Result<Response> {
        let runtime = self.runtime;
        Ok(match self.operation {
            Operation::Install {
                archive,
                size,
                force,
            } => {
                runtime.install(&archive, size, force)?;
                Response::Done
            }
            Operation::Backup { output } => {
                runtime.backup(&output)?;
                Response::Done
            }
            Operation::Start { foreground } => Response::Started {
                state: runtime.start(foreground)?,
                foreground: foreground || self.configured_foreground,
            },
            Operation::Restart { foreground } => Response::Started {
                state: runtime.restart(foreground)?,
                foreground: foreground || self.configured_foreground,
            },
            Operation::Stop => {
                runtime.stop()?;
                Response::Done
            }
            Operation::Enter { user, shell } => {
                Response::Status(runtime.enter(&user, shell.as_deref())?)
            }
            Operation::Run { command } => Response::Status(runtime.run(&command)?),
            Operation::Info => Response::Info(runtime.info()?),
            Operation::Pid => Response::Pid(runtime.pid()?),
            Operation::List | Operation::Scan => unreachable!("handled by the registry"),
        })
    }
}
