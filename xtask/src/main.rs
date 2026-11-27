// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

mod android;
mod build;
mod package;

use std::{env, path::Path, process::Command};

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Build and package KurumiContainerd", version)]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Compile the runtime (release builds enable servicectl).
    Build(build::BuildArgs),
    /// Package an existing target binary without rebuilding it.
    Package(package::PackageArgs),
    /// Compile the runtime and package it for distribution.
    Dist(package::PackageArgs),
    /// Package all four Android release binaries as a root module.
    AndroidModule(android::ModuleArgs),
    /// Validate a release tag and print the workspace version.
    ValidateRelease {
        #[arg(long)]
        tag: String,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Task::Build(args) => build::build(args.target, &args.flags),
        Task::Package(args) => package::package(&args),
        Task::Dist(args) => {
            ensure!(args.binary.is_none(), "dist does not accept --binary");
            package::validate(&args)?;
            build::build(Some(args.target), &args.flags)?;
            package::package(&args)
        }
        Task::AndroidModule(args) => android::package(&args),
        Task::ValidateRelease { tag } => {
            let version = release_version(&tag)?;
            println!("{version}");
            Ok(())
        }
    }
}

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is a workspace subdirectory")
}

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.current_dir(workspace()).env("LC_ALL", "C");
    command
}

fn cargo() -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(workspace());
    command
}

/// Propagates failed subprocesses so later build or packaging steps cannot run.
fn run(command: &mut Command) -> Result<()> {
    eprintln!("+ {command:?}");
    let status = command
        .status()
        .with_context(|| format!("Could not start {command:?}"))?;
    ensure!(status.success(), "{command:?} failed with {status}");
    Ok(())
}

fn capture(command: &mut Command) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("Could not start {command:?}"))?;
    ensure!(
        output.status.success(),
        "{command:?} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).context("Command output is not UTF-8")
}

/// Keeps versions safe for archive names and package metadata.
fn validate_version(version: &str) -> Result<()> {
    let (base, prerelease) = version
        .split_once('-')
        .map_or((version, None), |(base, suffix)| (base, Some(suffix)));
    let components: Vec<_> = base.split('.').collect();
    ensure!(
        components.len() == 3
            && components
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            && prerelease.is_none_or(|suffix| !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))),
        "Unsupported version: {version}"
    );
    Ok(())
}

fn release_version(tag: &str) -> Result<&'static str> {
    let version = env!("CARGO_PKG_VERSION");
    validate_version(version)?;
    ensure!(
        tag == format!("v{version}"),
        "Release tag must match Cargo version: v{version}"
    );
    Ok(version)
}

#[cfg(test)]
mod tests;
