// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::io::{self, BufRead, BufReader};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use kurumi_containerd_error::{ErrorContext as _, Result, bail, ensure};
use kurumi_containerd_helper::signal::{Signal, SignalActionFlags, SignalHandler, set_signal_handler};
use kurumi_containerd_helper::terminal::{PollEvents, poll};
use kurumi_containerd_host::process::ProcessHandle;
use uds::UnixStreamExt;

use crate::{
    Operation, Request, Response, SERVE_COMMAND, SOCKET_PATH, WireRequest, read_frame, write_frame,
};

/// Connects CLI or HTTP frontends to the daemon without sharing runtime process state.
#[derive(Clone)]
pub struct Client {
    executable: std::path::PathBuf,
    verbose: bool,
}

impl Client {
    #[must_use]
    pub fn new(executable: &Path, verbose: bool) -> Self {
        Self {
            executable: executable.to_path_buf(),
            verbose,
        }
    }

    /// Restarts the daemon with this client's executable, or starts it if absent.
    /// Running containers terminate when their owning daemon exits.
    ///
    /// # Errors
    /// Returns errors from peer verification, shutdown, or replacement startup.
    pub fn reload_daemon(&self) -> Result<()> {
        match UnixStream::connect(SOCKET_PATH) {
            Ok(stream) => {
                let peer = root_peer(&stream)?;
                let pid = peer.pid().context("daemon peer PID is unavailable")?;
                let daemon = ProcessHandle::open(i32::try_from(pid.get())?)?;
                // A reply on the original connection proves the peer was still alive
                // after pidfd_open, so a recycled PID cannot receive the signal.
                probe(&stream)?;
                daemon.send_signal(Signal::Terminate)?;
                ensure!(
                    daemon.wait_for_exit(Duration::from_secs(30))?,
                    "timed out waiting for the daemon to exit"
                );
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => {}
            Err(error) => {
                return Err(error).with_context(|| format!("failed to connect to {SOCKET_PATH}"));
            }
        }
        probe(&self.connect()?)
    }

    /// Sends one request, starting the daemon automatically when needed.
    ///
    /// # Errors
    /// Returns errors from startup, transport, or execution.
    pub fn request(&self, request: &Request) -> Result<Response> {
        ensure!(
            !matches!(request.operation, crate::Operation::Enter { .. }),
            "interactive login requires standard streams"
        );
        if let crate::Operation::Start { foreground } | crate::Operation::Restart { foreground } =
            request.operation
        {
            ensure!(
                !foreground
                    && !request
                        .config
                        .as_ref()
                        .is_some_and(|config| config.container.foreground),
                "foreground startup requires standard streams"
            );
        }
        self.send(request, false)
    }

    /// Sends a request with the calling process's standard streams attached.
    /// # Errors
    /// Returns startup, transport, or container operation failures.
    pub fn request_with_stdio(&self, request: &Request) -> Result<Response> {
        self.send(request, request.operation.uses_stdio())
    }

    fn send(&self, request: &Request, attach_stdio: bool) -> Result<Response> {
        let foreground = attach_stdio
            && matches!(
                request.operation,
                crate::Operation::Start { .. } | crate::Operation::Restart { .. }
            )
            && (matches!(
                request.operation,
                crate::Operation::Start { foreground: true }
                    | crate::Operation::Restart { foreground: true }
            ) || request
                .config
                .as_ref()
                .is_some_and(|config| config.container.foreground));
        let _signals = foreground.then(ForegroundSignals::install).transpose()?;
        let stream = self.connect()?;
        write_frame(
            &stream,
            &WireRequestRef {
                request,
                attach_stdio,
            },
        )
        .context("failed to send daemon request")?;
        if attach_stdio {
            let sent = stream
                .send_fds(
                    b"P",
                    &[
                        io::stdin().as_raw_fd(),
                        io::stdout().as_raw_fd(),
                        io::stderr().as_raw_fd(),
                    ],
                )
                .context("failed to send command stdio to daemon")?;
            ensure!(sent == 1, "failed to send standard stream marker");
        }
        if foreground {
            self.wait_foreground(&stream, request)?;
        }
        match read_frame(&stream).context("failed to receive daemon response")? {
            Response::Error(message) => bail!("{message}"),
            response => Ok(response),
        }
    }

    fn wait_foreground(&self, stream: &UnixStream, request: &Request) -> Result<()> {
        loop {
            if STOP_FOREGROUND.swap(false, Ordering::Relaxed) {
                let stop = Request {
                    config: request.config.clone(),
                    operation: crate::Operation::Stop,
                };
                if let Err(error) = self.request(&stop) {
                    tracing::debug!(%error, "foreground stop request failed");
                }
            }
            match poll([(stream.as_fd(), PollEvents::READABLE)], 100) {
                Ok([events]) if events.intersects(PollEvents::READABLE | PollEvents::HANGUP) => {
                    return Ok(());
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn connect(&self) -> Result<UnixStream> {
        match connect() {
            Ok(stream) => return Ok(stream),
            Err(error) => tracing::debug!(%error, socket = SOCKET_PATH, "starting daemon"),
        }
        let mut command = Command::new(&self.executable);
        if self.verbose {
            command.arg("--verbose");
        }
        let mut child = command
            .arg(SERVE_COMMAND)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to launch daemon")?;
        let mut line = String::new();
        let stdout = child
            .stdout
            .take()
            .context("daemon startup channel is missing")?;
        BufReader::new(stdout).read_line(&mut line)?;
        if line.trim_end() != "ready" {
            let status = child.wait()?;
            bail!("daemon failed to start ({status}): {}", line.trim_end());
        }
        let status = child.wait().context("failed to reap daemon launcher")?;
        ensure!(status.success(), "daemon launcher failed: {status}");
        let stream = connect().context("daemon announced readiness but connection failed")?;
        tracing::debug!(socket = SOCKET_PATH, "daemon ready");
        Ok(stream)
    }
}

fn connect() -> Result<UnixStream> {
    let stream = UnixStream::connect(SOCKET_PATH)
        .with_context(|| format!("failed to connect to {SOCKET_PATH}"))?;
    root_peer(&stream)?;
    Ok(stream)
}

fn root_peer(stream: &UnixStream) -> Result<uds::ConnCredentials> {
    let peer = stream.initial_peer_credentials()?;
    ensure!(peer.euid() == 0, "daemon socket peer is not root");
    Ok(peer)
}

fn probe(stream: &UnixStream) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    write_frame(
        stream,
        &WireRequest {
            request: Request {
                config: None,
                operation: Operation::List,
            },
            attach_stdio: false,
        },
    )?;
    match read_frame(stream).context("daemon did not answer readiness probe")? {
        Response::Containers(_) => Ok(()),
        Response::Error(message) => bail!("{message}"),
        response => bail!("unexpected daemon response: {response:?}"),
    }
}

#[derive(serde::Serialize)]
struct WireRequestRef<'a> {
    request: &'a Request,
    attach_stdio: bool,
}

static STOP_FOREGROUND: AtomicBool = AtomicBool::new(false);

struct ForegroundSignals;

impl ForegroundSignals {
    fn install() -> Result<Self> {
        extern "C" fn record(_: i32) {
            STOP_FOREGROUND.store(true, Ordering::Relaxed);
        }
        STOP_FOREGROUND.store(false, Ordering::Relaxed);
        for signal in [Signal::Interrupt, Signal::Terminate] {
            set_signal_handler(
                signal,
                SignalHandler::Handler(record),
                SignalActionFlags::empty(),
            )?;
        }
        Ok(Self)
    }
}

impl Drop for ForegroundSignals {
    fn drop(&mut self) {
        for signal in [Signal::Interrupt, Signal::Terminate] {
            let _ = set_signal_handler(signal, SignalHandler::Default, SignalActionFlags::empty());
        }
    }
}
