// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::fs::{self, File, Permissions};
use std::io::{self, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fs2::FileExt;
use kurumi_containerd_error::{ErrorContext as _, Result, RuntimeError, ensure};
use kurumi_containerd_helper::process::{ForkResult, StandardStream, WaitStatus};
use kurumi_containerd_helper::process::{duplicate_standard_stream, fork};
use kurumi_containerd_helper::process::{set_child_subreaper, setsid, waitpid};
use kurumi_containerd_helper::signal::{Signal, SignalActionFlags, SignalHandler, set_signal_handler};
use kurumi_containerd_helper::terminal::{PollEvents, poll, receive_stream_fds};
use kurumi_containerd_host::logs::{LogFile, socket_directory};
use tracing::level_filters::LevelFilter;
use uds::UnixStreamExt;

use crate::service::{Job, Service};
use crate::{Response, SOCKET_PATH, WireRequest, read_frame, write_frame};

static STOPPING: AtomicBool = AtomicBool::new(false);

extern "C" fn request_shutdown(_: i32) {
    STOPPING.store(true, Ordering::Relaxed);
}

/// Runs the single-threaded daemon process and acknowledges startup on stdout.
///
/// # Errors
/// Returns errors for missing privileges, socket setup, or listener failure.
pub fn serve(verbose: bool) -> Result<()> {
    if !detach()? {
        return Ok(());
    }
    let (listener, lock) = match bind() {
        Ok(Some(bound)) => bound,
        Ok(None) => {
            println!("ready");
            return Ok(());
        }
        Err(error) => {
            println!("{error:#}");
            return Err(error);
        }
    };
    let result = run_server(listener, &lock, verbose);
    if let Err(error) = &result {
        tracing::error!(error = %format_args!("{error:#}"), "daemon failed");
        println!("{error:#}");
    }
    let _ = fs::remove_file(SOCKET_PATH);
    result
}

fn run_server(listener: UnixListener, lock: &File, verbose: bool) -> Result<()> {
    let log = LogFile::Daemon.open()?;
    tracing_subscriber::fmt()
        .with_max_level(if verbose {
            LevelFilter::DEBUG
        } else {
            LevelFilter::INFO
        })
        .with_target(false)
        .with_ansi(false)
        .with_writer(log)
        .try_init()
        .map_err(|error| {
            RuntimeError::Message(format!("failed to initialize daemon logging: {error}"))
        })?;
    set_child_subreaper()?;
    let null = File::options().read(true).write(true).open("/dev/null")?;
    for signal in [Signal::Terminate, Signal::Interrupt, Signal::Hangup] {
        set_signal_handler(
            signal,
            SignalHandler::Handler(request_shutdown),
            SignalActionFlags::empty(),
        )?;
    }
    tracing::info!(
        pid = std::process::id(),
        socket = SOCKET_PATH,
        "daemon started"
    );
    println!("ready");
    io::stdout().flush()?;
    duplicate_standard_stream(&null, StandardStream::Input)?;
    duplicate_standard_stream(&null, StandardStream::Output)?;
    duplicate_standard_stream(&null, StandardStream::Error)?;
    drop(null);
    let result = event_loop(listener, lock);
    tracing::info!("daemon stopped");
    result
}

/// Double-forks before serving so the final process is adopted by init and
/// cannot acquire a controlling terminal as a session leader.
#[allow(unsafe_code)]
fn detach() -> Result<bool> {
    // SAFETY: the daemon entry point is single-threaded and all fork branches
    // either continue initialization or exit without starting additional threads.
    match unsafe { fork() }? {
        ForkResult::Parent { child } => {
            let status = loop {
                match waitpid(child, false) {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    result => break result?,
                }
            };
            ensure!(
                matches!(status, WaitStatus::Exited(_, 0)),
                "daemon detachment failed: {status:?}"
            );
            Ok(false)
        }
        ForkResult::Child => {
            setsid()?;
            // SAFETY: the first child remains single-threaded. Its parent branch
            // exits immediately, allowing init to adopt the final daemon process.
            match unsafe { fork() }? {
                ForkResult::Parent { .. } => std::process::exit(0),
                ForkResult::Child => Ok(true),
            }
        }
    }
}

fn bind() -> Result<Option<(UnixListener, File)>> {
    let path = Path::new(SOCKET_PATH);
    let lock = socket_directory()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match FileExt::try_lock_exclusive(&lock) {
            Ok(()) => break,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if UnixStream::connect(path).is_ok() {
                    return Ok(None);
                }
                if Instant::now() >= deadline {
                    return Err(error).context("previous daemon workers did not finish cleanup");
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error.into()),
        }
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_socket() && metadata.uid() == 0,
                "refusing to replace unexpected socket path {}",
                path.display()
            );
            match UnixStream::connect(path) {
                Ok(_) => return Ok(None),
                Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                    fs::remove_file(path)?;
                }
                Err(error) => return Err(error).context("failed to probe existing daemon socket"),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, Permissions::from_mode(0o600))?;
    Ok(Some((listener, lock)))
}

#[allow(unsafe_code)]
fn event_loop(listener: UnixListener, _lock: &File) -> Result<()> {
    let mut service = Service::default();
    let mut sequence = 0_u64;
    while !STOPPING.load(Ordering::Relaxed) {
        while matches!(
            waitpid(-1, true),
            Ok(WaitStatus::Exited(..) | WaitStatus::Signaled(..))
        ) {}
        match poll([(listener.as_fd(), PollEvents::READABLE)], 100) {
            Ok([events]) if events.contains(PollEvents::READABLE) => {}
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
        let (stream, _) = listener.accept()?;
        let credentials = stream.initial_peer_credentials()?;
        let client_pid = credentials.pid().map(std::num::NonZeroU32::get);
        if credentials.euid() != 0 {
            tracing::warn!(client_pid, uid = credentials.euid(), "request rejected");
            continue;
        }
        sequence = sequence.wrapping_add(1);
        let prepared = match prepare(&mut service, &stream, sequence, client_pid) {
            Ok(prepared) => prepared,
            Err(error) => {
                tracing::warn!(request_id = sequence, client_pid, error = %format_args!("{error:#}"), "request rejected");
                reply(&stream, &Response::Error(format!("{error:#}")));
                continue;
            }
        };
        let Prepared { job, stdio, log } = prepared;
        let job = match job {
            Ok(job) => job,
            Err(response) => {
                log.complete(&response);
                reply(&stream, &response);
                continue;
            }
        };
        // SAFETY: this process is single-threaded. The request worker owns its job
        // and can use the runtime's existing single-threaded fork and namespace paths.
        match unsafe { fork() } {
            Ok(ForkResult::Parent { .. }) => {}
            Ok(ForkResult::Child) => {
                drop(listener);
                for signal in [Signal::Terminate, Signal::Interrupt, Signal::Hangup] {
                    set_signal_handler(signal, SignalHandler::Default, SignalActionFlags::empty())?;
                }
                let attached = attach_stdio(&stdio);
                drop(stdio);
                let result = attached.and_then(|()| job.execute());
                let response = result.unwrap_or_else(|error| Response::Error(format!("{error:#}")));
                log.complete(&response);
                reply(&stream, &response);
                std::process::exit(0);
            }
            Err(error) => {
                let response = Response::Error(format!("failed to fork daemon worker: {error}"));
                log.complete(&response);
                reply(&stream, &response);
            }
        }
    }
    Ok(())
}

struct Prepared {
    job: Result<Job, Response>,
    stdio: Vec<OwnedFd>,
    log: RequestLog,
}

struct RequestLog {
    id: u64,
    client_pid: Option<u32>,
    operation: &'static str,
    container: Option<String>,
    started: Instant,
}

impl RequestLog {
    fn complete(&self, response: &Response) {
        let (status, exit_code, error) = match response {
            Response::Error(message) => ("error", None, Some(message.as_str())),
            Response::Status(code) => ("completed", Some(*code), None),
            _ => ("ok", None, None),
        };
        tracing::info!(
            request_id = self.id,
            client_pid = self.client_pid,
            operation = self.operation,
            container = self.container.as_deref(),
            status,
            exit_code,
            error,
            elapsed_ms = self.started.elapsed().as_millis(),
            "request completed"
        );
    }
}

fn prepare(
    service: &mut Service,
    stream: &UnixStream,
    id: u64,
    client_pid: Option<u32>,
) -> Result<Prepared> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let wire: WireRequest = read_frame(stream)?;
    let request = wire.request;
    let log = RequestLog {
        id,
        client_pid,
        operation: request.operation.name(),
        container: request
            .config
            .as_ref()
            .map(|config| config.container.name.clone()),
        started: Instant::now(),
    };
    tracing::info!(
        request_id = id,
        client_pid,
        operation = log.operation,
        container = log.container.as_deref(),
        "request received"
    );
    let stdio = if wire.attach_stdio {
        receive_stream_fds(stream, 3)?
    } else {
        Vec::new()
    };
    let job = match service.prepare(request) {
        Ok(job) => job,
        Err(error) => Err(Response::Error(format!("{error:#}"))),
    };
    Ok(Prepared { job, stdio, log })
}

fn attach_stdio(descriptors: &[OwnedFd]) -> Result<()> {
    if let [stdin, stdout, stderr] = descriptors {
        duplicate_standard_stream(stdin, StandardStream::Input)?;
        duplicate_standard_stream(stdout, StandardStream::Output)?;
        duplicate_standard_stream(stderr, StandardStream::Error)?;
    }
    Ok(())
}

fn reply(stream: &UnixStream, response: &Response) {
    if let Err(error) = write_frame(stream, response) {
        tracing::debug!(%error, "could not deliver daemon response");
    }
}
