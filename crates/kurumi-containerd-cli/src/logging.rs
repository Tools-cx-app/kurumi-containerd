use std::io::{self, IsTerminal, Write};

use kurumi_containerd_helper::process::effective_uid;
use kurumi_containerd_host::logs::LogFile;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::prelude::*;

const CLIENT_ACTIVITY: &str = "kurumi_client_activity";

fn diagnostic_stream() -> impl Write + IsTerminal {
    #[cfg(debug_assertions)]
    {
        io::stdout()
    }
    #[cfg(not(debug_assertions))]
    {
        io::stderr()
    }
}

pub(super) fn init(verbose: bool, command: &str, name: Option<&str>) {
    let (file, open_error) = if effective_uid() == 0 {
        match LogFile::Client.open() {
            Ok(file) => (Some(file), None),
            Err(error) => (None, Some(error)),
        }
    } else {
        (None, None)
    };
    let file_layer = file.map(|file| {
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_ansi(false)
            .with_writer(file)
    });
    let terminal_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_ansi(diagnostic_stream().is_terminal() && std::env::var_os("NO_COLOR").is_none())
        .with_writer(diagnostic_stream)
        .with_filter(filter_fn(|metadata| metadata.target() != CLIENT_ACTIVITY));
    tracing_subscriber::registry()
        .with(if verbose {
            LevelFilter::DEBUG
        } else {
            LevelFilter::INFO
        })
        .with(terminal_layer)
        .with(file_layer)
        .init();
    if let Some(error) = open_error {
        tracing::warn!(%error, "client file logging unavailable");
    }
    tracing::info!(target: CLIENT_ACTIVITY, command, name, pid = std::process::id(), "command started");
}
