use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

mod output;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use kurumi_containerd_config::{Config, ConfigPointer};
use kurumi_containerd_host::check::{HostCapabilities, HostCheck};
use kurumi_containerd_runtime::{ContainerRuntime, Runtime};
use tracing::level_filters::LevelFilter;

#[derive(Debug, Parser)]
#[command(version, about = "Privileged Linux container runtime")]
struct Cli {
    /// Select a configuration entry by its JSON management name.
    #[arg(long)]
    name: Option<String>,
    /// Include debug diagnostics on stderr.
    #[arg(short, long, default_value = "false")]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
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
    /// Recover validated running containers.
    Scan,
    /// Check host capabilities.
    Check,
    /// Open the interactive container manager.
    Tui,
}

fn main() -> std::process::ExitCode {
    let cli: Cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_max_level(if cli.verbose {
            LevelFilter::DEBUG
        } else {
            LevelFilter::INFO
        })
        .with_target(false)
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none())
        .init();
    match run(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error)
            if error
                .downcast_ref::<output::OutputError>()
                .is_some_and(output::OutputError::is_broken_pipe) =>
        {
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(error = %format_args!("{error:#}"), "command failed");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    if matches!(cli.command, Commands::Check) {
        return check();
    }
    if matches!(cli.command, Commands::Tui) {
        return kurumi_containerd_tui::run(&std::env::current_exe()?);
    }
    tracing::debug!(name = cli.name.as_deref(), "loading configuration registry");
    let pointers = ConfigPointer::load_home()?;
    let config_path = &ConfigPointer::select(&pointers, cli.name.as_deref())?.file;
    if let Commands::Install {
        archive,
        size,
        force,
    } = &cli.command
    {
        let config = Config::load_for_install(config_path)?;
        let target = config
            .container
            .rootfs
            .as_ref()
            .or(config.container.rootfs_image.as_ref())
            .context("rootfs target is not configured")?
            .clone();
        let runtime = Runtime::new(config)?;
        ContainerRuntime::install(&runtime, archive, *size, *force)?;
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
    let config = Config::load_persistent(config_path)?;
    let container_name = config.container.name.clone();
    let configured_foreground = config.container.foreground;
    let runtime = Runtime::new(config)?;
    let runtime: &dyn ContainerRuntime = &runtime;
    match cli.command {
        Commands::Start { foreground } => {
            let state = runtime.start(foreground)?;
            output::write(|out| {
                output::started(
                    out,
                    &state.name,
                    state.init_pid,
                    foreground || configured_foreground,
                )
            })?;
        }
        Commands::Stop => {
            runtime.stop()?;
            output::write(|out| writeln!(out, "Container {container_name} stopped"))?;
        }
        Commands::Restart { foreground } => {
            let state = runtime.restart(foreground)?;
            output::write(|out| {
                output::started(
                    out,
                    &state.name,
                    state.init_pid,
                    foreground || configured_foreground,
                )
            })?;
        }
        Commands::Enter { user } => {
            let status = runtime.enter(&user)?;
            if status != 0 {
                std::process::exit(status);
            }
        }
        Commands::Run { command } => {
            let status = runtime.run(&command)?;
            if status != 0 {
                std::process::exit(status);
            }
        }
        Commands::Info => {
            let info = runtime.info()?;
            output::write(|out| output::info(out, &info))?;
        }
        Commands::Pid => {
            let pid = runtime.pid()?;
            output::write(|out| output::pid(out, pid))?;
        }
        Commands::Show => {
            let states = runtime.list()?;
            output::write(|out| output::containers(out, &states))?;
        }
        Commands::Scan => {
            let states = runtime.scan()?;
            output::write(|out| output::recovered(out, &states))?;
        }
        Commands::Check => unreachable!("check is handled before loading configuration"),
        Commands::Tui => unreachable!("tui is handled before selecting a configuration"),
        Commands::Install { .. } => unreachable!("install is handled before loading configuration"),
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
        bail!("one or more required namespaces are unavailable");
    }
    Ok(())
}

fn print_check(capability: &str, available: bool, detail: &str) -> Result<()> {
    output::write(|out| output::check(out, capability, available, detail))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_has_help_text() {
        for command in Cli::command().get_subcommands() {
            assert!(
                command.get_about().is_some(),
                "{} has no help text",
                command.get_name()
            );
        }
    }

    #[test]
    fn exec_alias_parses_as_run() {
        let cli = Cli::try_parse_from(["kurumi-containerd", "exec", "sh", "-c", "true"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Run { command } if command == ["sh", "-c", "true"]
        ));
    }

    #[test]
    fn install_parses_local_archive_options() {
        let cli = Cli::try_parse_from([
            "kurumi-containerd",
            "install",
            "rootfs.tar.zst",
            "--size",
            "8G",
            "--force",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Install { archive, size: Some(size), force: true }
                if archive == Path::new("rootfs.tar.zst") && size == 8 * 1024_u64.pow(3)
        ));
    }

    #[test]
    fn rejects_config_options() {
        for flag in ["-c", "--config"] {
            assert!(Cli::try_parse_from(["kurumi-containerd", flag, "old.toml", "start"]).is_err());
        }
        assert!(Cli::try_parse_from(["kurumi-containerd", "start"]).is_ok());
    }

    #[test]
    fn parses_binary_sizes() {
        assert_eq!(parse_size("512M").unwrap(), 512 * 1024_u64.pow(2));
        assert_eq!(parse_size("8G").unwrap(), 8 * 1024_u64.pow(3));
        assert_eq!(parse_size("16GiB").unwrap(), 16 * 1024_u64.pow(3));
    }

    #[test]
    fn rejects_invalid_sizes() {
        for value in ["0", "-1G", "1PB", "18446744073709551615T"] {
            assert!(parse_size(value).is_err(), "accepted {value}");
        }
    }
}
