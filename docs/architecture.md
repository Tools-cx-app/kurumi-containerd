# Runtime architecture

[[zh](architecture_CN.md) / [en](architecture.md)]

[Documentation](README.md)

KurumiContainerd separates command presentation, container policy, and host
resources across seven workspace crates. This page describes those boundaries,
resource ownership, and the checks that make persisted state usable. It does not
describe a security boundary for hostile workloads.

## Workspace boundaries

```text
crates/
  kurumi-containerd-cli/      command parsing and output
  kurumi-containerd-config/   strict JSON schema, path resolution, and validation
  kurumi-containerd-error/    shared error types, result aliases, and context
  kurumi-containerd-helper/   target-aware Linux and Android syscall wrappers
  kurumi-containerd-host/     host resources and platform integration
  kurumi-containerd-runtime/  lifecycle, isolation, and state
  kurumi-containerd-tui/      ratatui manager and CLI action dispatch
```

The CLI package and executable are named `kurumi-containerd`; the other crates
provide internal implementation boundaries. Dependencies run toward lower-level
services: CLI uses TUI, runtime, configuration, and host; TUI uses runtime and
configuration; runtime uses host, configuration, helper, and error; host uses
configuration, helper, and error; configuration uses error. Helper and error do
not depend on other workspace crates. Host never imports runtime lifecycle state
or init-family policy: runtime passes it network-state paths and shutdown callbacks.

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

Callers use `kurumi_containerd_config`, without depending on this private layout.
The registry implementation is `registry.rs`, not the former `pointer.rs` name.

## Lifecycle

Starting validates the configuration, takes the container's lifecycle lock,
prepares its rootfs, allocates long-lived resources, and creates a monitor.
For each boot generation, an intermediate process creates fresh PID and optional
network namespaces before forking init.

The monitor owns rootfs mounts, the console PTY, cgroups, and applicable Android
SELinux state across internal container reboots. Init and monitor identities
include the host boot ID, process start time, and PID namespace inode; a numeric
PID alone is never sufficient evidence that the expected process is still alive.
Init signaling and waiting use pidfds when available.

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
| `runtime/state/store.rs` | Private `StateStore`: trusted persistence, state paths, and locks |
| `runtime/state/info.rs` | Status and resource usage |
| `runtime/execute.rs` | Namespace entry, command execution, and interactive login |
| `runtime/install.rs` | Lifecycle coordination for rootfs installation |
| `runtime/backup.rs` | Root checks, lifecycle locking, and stopped-only rootfs export |
| `container/environment.rs` | Deterministic init and session environments |
| `container/init.rs` | Init-family detection and shutdown protocols |
| `container/security.rs` | Seccomp and protected kernel views |

In the current source tree, query and persistence are files (`query.rs` and
`state/store.rs`), not `query/` or `state/store/` module directories.

Host resources live in the host crate's `src/`:

| Module | Responsibility |
| --- | --- |
| `process.rs` | Pidfd handles and procfs process identity helpers |
| `terminal.rs` | PTY allocation, descriptor passing, and console proxying |
| `rootfs/mod.rs`, `rootfs/install.rs` | Rootfs preparation, backup entry points, and archive installation |
| `archive.rs`, `archive/backup.rs` | Private archive extraction and streaming export |
| `network/mod.rs` | Veth, bridges, NAT, forwarding, and rollback |
| `network/state.rs` | Network state and NAT lease persistence |
| `cgroup.rs` | Cgroup v1/v2 limits and hierarchy detection |
| `android.rs` | Android host integration, exposed only on Android targets |

Host resource APIs take the configuration data they need: rootfs paths for
`Rootfs`, and the container name, network mode, and network options for network
setup. Config-taking constructors remain compatibility adapters for callers
outside runtime. Resource ownership stays in the parent modules; network
persistence is separate from network setup and cleanup orchestration.

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
`ContainerQuery`, and dispatches actions through either a graphical terminal or
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

Runtime files and directories have restrictive ownership and permissions.
State updates use no-follow opens, random temporary files, fsync, and atomic
rename. Recovery requires a protected host recovery record and matching identity
markers under `/run/kurumi-containerd` inside the container; scanning a live PID 1
without that record is not enough.

State publication, removal, and recovery commits share a per-container
persistence lock in the state directory. While holding it, recovery compares the
original record again and discards candidates changed or removed by lifecycle
work. This lock is separate from the lifecycle lock, so scans during install or
backup do not re-enter the lifecycle lock or disrupt reboot coordination.
`StateStore` owns these paths, locks, and trusted persistence operations;
process identity and recovery policy stay in the surrounding state module.

The runtime installs a classic-BPF seccomp filter and can protect kernel views
with read-only mounts and bind masks. Container root still retains its
capabilities, and privileged container mechanisms share the host kernel. These
controls do not turn KurumiContainerd into a sandbox for hostile workloads.
Use trusted root filesystems and commands, and review the
[current limitations](status.md#current-limitations) before deployment.
