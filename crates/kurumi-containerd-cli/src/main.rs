// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

use clap::{Parser, Subcommand};
use kurumi_containerd_config::ConfigEntry;
use kurumi_containerd_daemon::{Client, Operation, Request, Response, serve};
use kurumi_containerd_error::{ErrorContext as _, Result, bail};
use kurumi_containerd_host::check::{HostCapabilities, HostCheck};
use tracing::level_filters::LevelFilter;

#[cfg(target_os = "android")]
mod android;
mod output;

#[cfg(debug_assertions)]
fn logger_writer() -> io::Stdout {
    io::stdout()
}

#[cfg(not(debug_assertions))]
fn logger_writer() -> io::Stderr {
    io::stderr()
}

#[cfg(debug_assertions)]
fn logger_is_terminal() -> bool {
    io::stdout().is_terminal()
}

#[cfg(not(debug_assertions))]
fn logger_is_terminal() -> bool {
    io::stderr().is_terminal()
}

#[derive(Debug, Parser)]
#[command(version, about = "Privileged Linux container runtime")]
struct Cli {
    /// Select a configuration entry by its container name.
    #[arg(long)]
    name: Option<String>,
    /// Include debug diagnostics on the build-selected logger stream.
    #[arg(short, long, default_value = "false")]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run the daemon process.
    #[command(name = "__daemon", hide = true)]
    Daemon,
    /// Install a local rootfs archive into the configured target.
    Install {
        /// Local tar or ZIP rootfs archive.
        archive: PathBuf,
        /// Sparse ext4 image size, required only for `rootfs_image` targets.
        #[arg(long, value_parser = parse_size)]
        size: Option<u64>,
        /// Atomically replace an existing rootfs target.
        #[arg(long)]
        force: bool,
    },
    /// Back up a stopped container's rootfs, selecting its container name.
    Backup {
        /// Container ID: the exact name in $HOME/.kurumi-containerd/config.json.
        container_id: String,
        /// New archive path ending in .zst, .xz, .zip or .tar.
        output: PathBuf,
    },
    /// Start the configured container.
    Start {
        /// Attach the container console to this terminal.
        #[arg(short, long)]
        foreground: bool,
    },
    /// Stop the configured container.
    Stop,
    /// Restart the configured container.
    Restart {
        /// Attach the container console to this terminal.
        #[arg(short, long)]
        foreground: bool,
    },
    /// Open an interactive login in the container.
    Enter {
        /// Login user.
        #[arg(default_value = "root")]
        user: String,
        /// Login shell command with an absolute container path and optional arguments; bypasses su.
        #[arg(long, value_name = "COMMAND")]
        shell: Option<String>,
    },
    /// Run a command in the container.
    #[command(visible_alias = "exec")]
    Run {
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// Show container status and resource usage.
    Info,
    /// Print the container init PID.
    Pid,
    /// List running containers.
    Show,
    /// Validate containers tracked by daemon memory.
    Scan,
    /// Check host capabilities.
    Check,
    /// Open the interactive container manager.
    #[cfg(feature = "tui")]
    Tui,
}

fn main() -> std::process::ExitCode {
    let cli: Cli = Cli::parse();
    if !matches!(cli.command, Commands::Daemon) {
        tracing_subscriber::fmt()
            .with_max_level(if cli.verbose {
                LevelFilter::DEBUG
            } else {
                LevelFilter::INFO
            })
            .with_target(false)
            .with_writer(logger_writer)
            .with_ansi(logger_is_terminal() && std::env::var_os("NO_COLOR").is_none())
            .init();
    }
    #[cfg(target_os = "android")]
    if !matches!(cli.command, Commands::Daemon) {
        android::wrap_stdio();
    }
    match run(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) if output::is_broken_pipe(&error) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error = %format_args!("{error:#}"), "command failed");
            std::process::ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run(cli: Cli) -> Result<()> {
    if matches!(cli.command, Commands::Check) {
        return check();
    }
    #[cfg(feature = "tui")]
    if matches!(cli.command, Commands::Tui) {
        return kurumi_containerd_tui::run(&std::env::current_exe()?);
    }
    if matches!(cli.command, Commands::Daemon) {
        return serve(cli.verbose);
    }
    let executable = std::env::current_exe()?;
    let client = Client::new(&executable, cli.verbose);
    if matches!(cli.command, Commands::Show | Commands::Scan) {
        let scan = matches!(cli.command, Commands::Scan);
        let response = client.request_with_stdio(&Request {
            config: None,
            operation: if scan {
                Operation::Scan
            } else {
                Operation::List
            },
        })?;
        let Response::Containers(states) = response else {
            bail!("unexpected daemon response: {response:?}");
        };
        output::write(|out| {
            if scan {
                output::recovered(out, &states)
            } else {
                output::containers(out, &states)
            }
        })?;
        return Ok(());
    }
    if let Commands::Install {
        archive,
        size,
        force,
    } = cli.command
    {
        let entry = ConfigEntry::prepare_install_home(cli.name.as_deref(), size.is_some())?;
        let config = entry.load_for_install()?;
        let target = config
            .container
            .rootfs
            .as_ref()
            .or(config.container.rootfs_image.as_ref())
            .context("rootfs target is not configured")?
            .clone();
        let archive = std::path::absolute(archive)?;
        client.request_with_stdio(&Request {
            config: Some(config),
            operation: Operation::Install {
                archive: archive.clone(),
                size,
                force,
            },
        })?;
        output::write(|out| {
            writeln!(
                out,
                "Rootfs installed: {} -> {}",
                archive.display(),
                target.display()
            )
        })?;
        return Ok(());
    }
    tracing::debug!(name = cli.name.as_deref(), "loading configuration registry");
    let entries = ConfigEntry::load_home()?;
    if let Commands::Backup {
        container_id,
        output,
    } = cli.command
    {
        if cli.name.is_some() {
            bail!("backup selects its container by CONTAINER_ID; do not supply --name");
        }
        let entry = ConfigEntry::select(&entries, Some(&container_id))?;
        let config = entry.load_config()?;
        client.request_with_stdio(&Request {
            config: Some(config),
            operation: Operation::Backup {
                output: std::path::absolute(&output)?,
            },
        })?;
        output::write(|out| {
            writeln!(
                out,
                "Container {container_id} backed up: {}",
                output.display()
            )
        })?;
        return Ok(());
    }
    let entry = ConfigEntry::select(&entries, cli.name.as_deref())?;
    let config = if matches!(
        cli.command,
        Commands::Start { .. } | Commands::Restart { .. }
    ) {
        entry.load_persistent()?
    } else {
        entry.load_config()?
    };
    let operation = match cli.command {
        Commands::Start { foreground } => Operation::Start { foreground },
        Commands::Stop => Operation::Stop,
        Commands::Restart { foreground } => Operation::Restart { foreground },
        Commands::Enter { user, shell } => Operation::Enter { user, shell },
        Commands::Run { command } => Operation::Run { command },
        Commands::Info => Operation::Info,
        Commands::Pid => Operation::Pid,
        _ => unreachable!("command handled before loading configuration"),
    };
    let response = client.request_with_stdio(&Request {
        config: Some(config),
        operation,
    })?;
    match response {
        Response::Done => output::write(|out| writeln!(out, "Container {} stopped", entry.name))?,
        Response::Started { state, foreground } => {
            output::write(|out| output::started(out, &state.name, state.init_pid, foreground))?;
        }
        Response::Status(status) if status != 0 => std::process::exit(status),
        Response::Status(_) => {}
        Response::Info(info) => output::write(|out| output::info(out, &info))?,
        Response::Pid(pid) => output::write(|out| output::pid(out, pid))?,
        response => bail!("unexpected daemon response: {response:?}"),
    }
    Ok(())
}

fn parse_size(value: &str) -> Result<u64, String> {
    let split = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number = number
        .parse::<u64>()
        .map_err(|_| "size must begin with a positive integer".to_owned())?;
    if number == 0 {
        return Err("size must be greater than zero".to_owned());
    }
    let multiplier = match unit.to_ascii_lowercase().as_str() {
        "" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024_u64.pow(2),
        "g" | "gb" | "gib" => 1024_u64.pow(3),
        "t" | "tb" | "tib" => 1024_u64.pow(4),
        _ => return Err(format!("unsupported size unit: {unit}")),
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| "size is too large".to_owned())
}

fn check() -> Result<()> {
    output::write(|out| writeln!(out, "Host: {}", std::env::consts::OS))?;
    let report = HostCapabilities.check()?;
    for capability in &report.capabilities {
        print_check(&capability.name, capability.available, &capability.detail)?;
    }
    if !report.required_namespaces_available() {
        bail!("required mount or UTS namespace is unavailable");
    }
    Ok(())
}

fn print_check(capability: &str, available: bool, detail: &str) -> Result<()> {
    output::write(|out| output::check(out, capability, available, detail))?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/tests.rs"]
mod tests;
