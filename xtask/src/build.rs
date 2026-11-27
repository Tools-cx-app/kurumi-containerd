// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{fmt, process::Command, str::FromStr};

use anyhow::{Result, ensure};
use clap::Args;

use crate::{cargo, run};

pub const ANDROID_ABIS: [&str; 4] = ["arm64-v8a", "armeabi-v7a", "x86_64", "x86"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub triple: &'static str,
    pub abi: Option<&'static str>,
}

impl FromStr for Target {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (triple, abi) = match value {
            "x86_64-unknown-linux-gnu" => ("x86_64-unknown-linux-gnu", None),
            "aarch64-unknown-linux-gnu" => ("aarch64-unknown-linux-gnu", None),
            "armv7-unknown-linux-gnueabihf" => ("armv7-unknown-linux-gnueabihf", None),
            "riscv64gc-unknown-linux-gnu" => ("riscv64gc-unknown-linux-gnu", None),
            "x86_64-unknown-linux-musl" => ("x86_64-unknown-linux-musl", None),
            "aarch64-unknown-linux-musl" => ("aarch64-unknown-linux-musl", None),
            "aarch64-linux-android" | "arm64-v8a" => ("aarch64-linux-android", Some("arm64-v8a")),
            "armv7-linux-androideabi" | "armeabi-v7a" => {
                ("armv7-linux-androideabi", Some("armeabi-v7a"))
            }
            "x86_64-linux-android" | "x86_64" => ("x86_64-linux-android", Some("x86_64")),
            "i686-linux-android" | "x86" => ("i686-linux-android", Some("x86")),
            _ => return Err(format!("Unsupported Linux target or Android ABI: {value}")),
        };
        Ok(Self { triple, abi })
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.triple)
    }
}

impl Target {
    pub fn platform(self) -> &'static str {
        if self.abi.is_some() {
            "android"
        } else {
            "linux"
        }
    }

    pub fn native_arches(self) -> Option<(&'static str, &'static str)> {
        match self.triple {
            "x86_64-unknown-linux-gnu" => Some(("amd64", "x86_64")),
            "aarch64-unknown-linux-gnu" => Some(("arm64", "aarch64")),
            "armv7-unknown-linux-gnueabihf" => Some(("armhf", "armv7hl")),
            "riscv64gc-unknown-linux-gnu" => Some(("riscv64", "riscv64")),
            _ => None,
        }
    }
}

#[derive(Args, Default)]
pub struct BuildFlags {
    /// Use the release profile and enable shared PID/IPC support.
    #[arg(short, long)]
    pub release: bool,
    /// Include the terminal manager (Linux only).
    #[arg(long)]
    pub with_tui: bool,
    /// Print detailed Cargo output.
    #[arg(short, long)]
    pub verbose: bool,
}

impl BuildFlags {
    pub fn profile(&self) -> &'static str {
        if self.release { "release" } else { "debug" }
    }

    pub fn validate(&self, target: Option<Target>) -> Result<()> {
        ensure!(
            !self.with_tui || target.is_none_or(|target| target.abi.is_none()),
            "--with-tui is supported only for Linux targets"
        );
        Ok(())
    }
}

#[derive(Args)]
pub struct BuildArgs {
    /// Linux target triple, Android target triple, or Android ABI; defaults to the host.
    #[arg(long)]
    pub target: Option<Target>,
    #[command(flatten)]
    pub flags: BuildFlags,
}

pub fn build(target: Option<Target>, flags: &BuildFlags) -> Result<()> {
    run(&mut build_command(target, flags)?)
}

pub fn build_command(target: Option<Target>, flags: &BuildFlags) -> Result<Command> {
    flags.validate(target)?;
    let mut command = cargo();
    if let Some(abi) = target.and_then(|target| target.abi) {
        command.args(["ndk", "--platform", "21", "--target", abi]);
    }
    command.arg("build");
    if flags.with_tui {
        command.args(["--package", "kurumi-containerd"]);
    } else {
        command.args(["--workspace", "--exclude", "xtask"]);
    }
    if let Some(target) = target.filter(|target| target.abi.is_none()) {
        command.args(["--target", target.triple]);
    }
    if flags.release {
        command.args(["--release", "--features", "kurumi-containerd/servicectl"]);
    }
    if flags.with_tui {
        command.args(["--features", "kurumi-containerd/tui"]);
    }
    if flags.verbose {
        command.arg("--verbose");
    }
    Ok(command)
}
