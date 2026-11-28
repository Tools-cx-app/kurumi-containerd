# Runtime architecture

[简体中文](../zh/architecture.md) | [English](../en/architecture.md)

[Documentation](README.md)

KurumiContainerd separates command presentation, container policy, and host
resources across eight workspace crates. This page describes those boundaries,
resource ownership, daemon communication, and in-memory process identity checks.

## Workspace boundaries

```text
crates/
  kurumi-containerd-cli/      command parsing and output
  kurumi-containerd-config/   strict JSON schema, path resolution, and validation
  kurumi-containerd-error/    shared error types, result aliases, and context
  kurumi-containerd-daemon/   UDS client/server, autostart, and request dispatch
  kurumi-containerd-helper/   target-aware Linux and Android syscall wrappers
  kurumi-containerd-host/     host resources and platform integration
  kurumi-containerd-runtime/  lifecycle, isolation, and state
  kurumi-containerd-tui/      ratatui manager and CLI action dispatch
```

The CLI package and executable are named `kurumi-containerd`; the other crates
provide internal implementation boundaries. Dependencies run toward lower-level
services: CLI uses TUI, daemon, runtime output types, configuration, and host; TUI uses daemon, runtime output types, and
configuration; daemon uses runtime, configuration, helper, and error; runtime uses host, configuration, helper, and error; host uses
configuration, helper, and error; configuration uses error. Helper and error do
not depend on other workspace crates. Host never imports runtime lifecycle state
or init-family policy. Runtime passes shared ownership records and shutdown callbacks.

The CLI's `tui` feature is disabled by default. Building
`cargo build -p kurumi-containerd` leaves out the optional TUI dependency and its
terminal dependencies; `--features tui` includes them and the `tui` subcommand.
The TUI is still a workspace member, so workspace-wide builds and tests include it.

The error crate owns `ConfigError`, `RuntimeError`, result aliases, context, and
error macros. Configuration and runtime re-export their existing error APIs.
Host and runtime share `RuntimeError`, allowing policy callbacks to cross that
boundary without conversion; helper keeps low-level I/O errors. Error's default
`runtime` feature enables runtime errors, context, macros, and optional
procfs/JSON/ZIP error dependencies. The workspace disables that default;
configuration inherits the dependency while host and runtime explicitly enable
`runtime`. Configuration-only builds therefore do not pull procfs or ZIP through
the error crate. Other consumers that disable defaults must enable `runtime` to
use runtime errors.

Public interfaces follow the same split. Configuration exposes the mutating
validation flow through `Validate`; runtime exposes user-facing operations through
`ContainerRuntime` and read-only status access through `ContainerQuery`; host
exposes `HostCheck` and a typed capability report. Namespace, mount, cgroup,
process, and state implementation details stay on concrete types. CLI formats
the host report. CLI and TUI also format structured `ContainerInfo` fields in
their own presentation layers; runtime's `Display` and `display_lines` remain
compatibility APIs rather than the home for new formatting behavior.

## Configuration modules

The configuration crate exports schema types from `lib.rs`. Loading and
validation remain together, with separate modules for environment parsing and
registry selection:

```text
src/
  lib.rs          public configuration API
  model.rs        serializable configuration schema and defaults
  config.rs       loading, path resolution, and validation
  environment.rs  environment assignment parsing
  registry.rs     configuration registry loading and selection
```

Callers use the public `kurumi_containerd_config` API without depending on this private layout.

## Lifecycle

A start operation validates configuration, locks the container identity,
prepares the root filesystem, allocates long-lived resources, and creates a
monitor. Each boot generation uses an intermediate worker to create mount/UTS
and optional network namespaces before forking init. Default `isolated` mode also
creates PID/IPC namespaces; explicit `shared-pid-ipc` inherits those spaces.

Init detection resolves the actual `/sbin/init` inside the rootfs, including
container-relative symlinks; installing another init package does not change the
selected family. Systemd shutdown runs the guest `systemctl --no-block
--job-mode=replace-irreversibly start halt.target` in the container's mount/PID
namespaces and root, avoiding host/guest libc real-time signal differences.
The control client has a five-second timeout. If the client is missing or fails,
stop reports a warning and retains the configured wait before forced termination.

The monitor owns resources that must survive an internal container reboot:
rootfs mounts, console PTY, cgroup, and Android SELinux state where applicable.
It records init and monitor identities using the host boot ID and process start
times. Isolated mode additionally requires a PID namespace inode. Shared mode
records the actual mode in memory, worker identity, mount namespace and exclusive cgroup
path; missing PID/IPC namespace interfaces are allowed.

Stop and restart share the per-container lifecycle lock. Stop additionally holds
a separate stop-request lock while waiting for the monitor. An internal reboot
waits for ordinary lifecycle-lock holders and cancels only when a stop request
is active. Closing the stop-request descriptor clears the request, including
when its caller crashes.

Commands entering a running container capture init's actual network and cgroup
namespaces and cgroup membership instead of reconstructing them from the current
JSON. They open cgroup directories through the validated monitor's mount view,
which keeps privately bootstrapped hierarchies reachable, and join execution
leaves beneath the captured groups without changing resource limits. Editing
network or resource configuration affects the next start, not commands entering
the current generation.

Shared mode unconditionally allocates a dedicated cgroup v2 subtree, even without
limits. Init, services and exec leaves belong to it; the monitor and generation
worker stay outside. The worker is a child subreaper and keeps an open cgroup
directory plus a monitor-lifetime pipe. Monitor EOF triggers generation cleanup.
After graceful supervisor shutdown, cleanup terminates remaining recursive cgroup
members and waits for emptiness and child reaping before publishing a stopped
state or allowing another generation. `cgroup.kill` is optional: older kernels
use repeated enumeration and trusted process handles. Cleanup errors preserve
recoverable records rather than reporting a successful stop.

The shared rootfs sees only its delegated cgroup subtree. It retains private
mount/UTS and runtime directories but sees host PIDs and shares SysV IPC/POSIX
message queues. Both init and entered workloads receive reboot syscall denial.
SIGHUP is terminal for a shared supervisor; it is not a namespace reboot request.
The [service-stack example](servicectl.md) owns Dinit/s6 startup and shutdown in
the rootfs, with no Go dependency in Kurumi's Cargo graph.


## Runtime modules

Runtime code is grouped by responsibility:

```text
src/
  container/  behavior and policy applied inside the container
  runtime/    lifecycle orchestration, execution, state, and supervision
```

Paths below are relative to the runtime crate's `src/`:

| Module | Responsibility |
| --- | --- |
| `runtime/lifecycle.rs` | Start, monitor, boot generations, shutdown, and cleanup |
| `runtime/boot.rs` | Namespace-internal mounts, pivot, nested sandbox mounts, environment, and init execution |
| `runtime/supervisor.rs` | Wait/retry, signal policy, and child supervision helpers |
| `runtime/query.rs` | Read-only `ContainerQuery` interface for info, PID, and live-container listing |
| `runtime/state/mod.rs` | Public state types, process identity, and recovery policy |
| `runtime/state/store.rs` | Private `StateStore`: anonymous memory records and lifecycle locks |
| `runtime/state/info.rs` | Status and resource usage |
| `runtime/execute.rs` | Namespace entry, command execution, and interactive login |
| `runtime/install.rs` | Lifecycle coordination for rootfs installation |
| `runtime/backup.rs` | Root checks, lifecycle locking, and stopped-only rootfs export |
| `container/environment.rs` | Deterministic init and session environments |
| `container/init.rs` | Init-family detection and shutdown protocols |
| `container/security.rs` | Seccomp and protected kernel views |

Host resources live in the host crate's `src/`:

| Module | Responsibility |
| --- | --- |
| `process.rs` | Pidfd handles and procfs process identity helpers |
| `terminal.rs` | PTY allocation, descriptor passing, and console proxying |
| `rootfs/mod.rs`, `rootfs/install.rs` | Rootfs preparation, backup entry points, and archive installation |
| `archive.rs`, `archive/backup.rs` | Private archive extraction and streaming export |
| `network/mod.rs` | Veth, bridges, NAT, forwarding, and rollback |
| `network/state.rs` | In-memory network ownership and NAT leases |
| `cgroup.rs` | Cgroup v1/v2 limits and hierarchy detection |
| `android.rs` | Android host integration, exposed only on Android targets |

Host resource APIs take the configuration data they need: rootfs paths for
`Rootfs`, and the container name, network mode, and network options for network
setup. Config-taking constructors remain compatibility adapters for callers
outside runtime. Resource ownership stays in the parent modules; network
ownership state is separate from network setup and cleanup orchestration.

The terminal proxy uses helper's safe `terminal::poll` wrapper for descriptor
polling. Helper handles target-native `pollfd`, event flags, and a checked
`nfds_t` count, returning events and interrupted waits. Host owns the 100 ms
wait, interruption retry, input buffering, shutdown callbacks, and background
PTY drain policy. Background output must keep being drained even when discarded,
otherwise a full PTY buffer can block init or its services.

Directory modules group related production submodules: host `network` and
`rootfs`, and runtime `container`, `runtime`, and `runtime/state`. Cohesive leaf
modules use `<module>.rs`; terminal, configuration, and TUI rendering helpers
stay together when they share lifecycle or rendering state. Test files retain
their locations and visibility through explicit paths where needed.

### TUI backend and presentation

The TUI separates rendering, input state, and action validation from its private
backend. `registry.rs` owns only the `Entry` model. `backend.rs` loads and
refreshes configurations without persisting UUIDs, obtains runtime status through
the daemon UDS client, and dispatches actions through either a graphical terminal or
a captured subprocess of the same CLI executable.

`CommandTask` owns both the child process and its output file. Polling returns
the exit status and at most 1 MiB of combined stdout/stderr, decoded as lossy
UTF-8. Temporary-file and polling I/O errors propagate; spawn failures remain
recoverable UI feedback. The app owns one optional task, busy state, refresh
timing, and feedback, rather than constructing commands or reading captures.
The backend preserves the CLI arguments and terminal-selection behavior described
in [CLI usage](usage.md#terminal-manager).

## Nested user namespaces

With `container.security.allow_user_namespaces` enabled, `runtime/boot.rs`
mounts `/run` before the ordinary container filesystem setup. After `pivot_root`,
it creates two additional filesystem instances:

| Mount | Flags |
| --- | --- |
| `/run/kurumi-containerd/proc` | Writable procfs with `nosuid,nodev,noexec` |
| `/run/kurumi-containerd/sys` | Read-only sysfs with `ro,nosuid,nodev,noexec` |

The regular `/proc` and `/sys` retain their configured hardening and masks.
The additional instances satisfy kernel visibility requirements when Docker,
Podman, or another nested sandbox mounts procfs/sysfs from an unprivileged user
namespace. The pristine proc view can expose host kernel sysctls. Enable this
option only for trusted containers; it is not a hostile-workload boundary.

## State and trust

The CLI and TUI use the UDS client in `kurumi-containerd-daemon`. The socket is
`/var/kurumi/socket.sock` on Linux and `/dev/kurumi/socket.sock` on Android.
An unavailable socket triggers automatic daemon startup and a client log message.
The daemon double-forks, creates a separate session, and is adopted by PID 1.
Its standard streams point to `/dev/null`. In the socket directory, root CLI
processes append diagnostics to `client.log`, the daemon writes request and
runtime events to `daemon.log`, and monitors append background console records
to `container.log`. Monitors keep draining the PTY if logging fails.

The daemon retains `RuntimeRegistry` and one private `StateStore` per container.
Anonymous memfd records share state with single-threaded request workers and
container monitors. State publication uses an inactive slot followed by a commit
selector under an anonymous lock. Lifecycle and stop-request locks are separate.
Runtime state, recovery metadata, NAT leases, and Android SELinux ownership stay
in memory. Configuration JSON and rootfs data keep their existing storage rules.

`show` and `scan` validate the daemon's tracked records. Old state and recovery
files are ignored. Each generation retains its actual isolation mode and trusted
process identities, including boot ID, start times, namespace identity, and shared
cgroup membership. A daemon exit triggers container termination and monitor
cleanup. A new daemon starts with an empty registry after old monitors release
the instance lock.

The typed `Request`, `Operation`, `Response`, and owned `Client` form the
frontend interface. An Axum handler can clone the client and run its blocking
`request` call in `tokio::task::spawn_blocking`. Interactive CLI operations use
`request_with_stdio` and transfer descriptors through the `uds` crate. The daemon
executes namespace and fork operations in dedicated single-threaded processes.

The runtime installs a classic-BPF seccomp filter and can protect kernel views
with read-only mounts and bind masks. Container root still retains its
capabilities, and privileged container mechanisms share the host kernel. These
controls do not turn KurumiContainerd into a sandbox for hostile workloads.
Use trusted root filesystems and commands, and review the
[current limitations](status.md#current-limitations) before deployment.
