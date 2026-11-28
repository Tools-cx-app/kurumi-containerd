// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

//! Local daemon transport and typed container operations.

use kurumi_containerd_error::Result;

mod client;
mod server;
mod service;

pub use client::Client;
pub use server::serve;
pub use service::{Job, Operation, Request, Response, Service};

#[cfg(target_os = "android")]
pub const SOCKET_PATH: &str = "/dev/kurumi/socket.sock";
#[cfg(target_os = "linux")]
pub const SOCKET_PATH: &str = "/var/kurumi/socket.sock";

/// Internal CLI entry point used by the autostart client.
pub const SERVE_COMMAND: &str = "__daemon";

const MAX_FRAME: usize = 4 * 1024 * 1024;

fn read_frame<T>(mut reader: impl std::io::Read) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    kurumi_containerd_error::ensure!(length <= MAX_FRAME, "daemon message exceeds size limit");
    let mut data = vec![0; length];
    reader.read_exact(&mut data)?;
    Ok(serde_json::from_slice(&data)?)
}

fn write_frame(mut writer: impl std::io::Write, value: &impl serde::Serialize) -> Result<()> {
    let data = serde_json::to_vec(value)?;
    kurumi_containerd_error::ensure!(data.len() <= MAX_FRAME, "daemon message exceeds size limit");
    writer.write_all(&u32::try_from(data.len())?.to_be_bytes())?;
    writer.write_all(&data)?;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WireRequest {
    request: Request,
    attach_stdio: bool,
}
