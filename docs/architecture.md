# Runtime architecture

KurumiContainerd is a Cargo workspace with seven crates:

```text
crates/
  kurumi-containerd-cli/      command parsing and output
  kurumi-containerd-config/   strict TOML schema, path resolution, and validation
  kurumi-containerd-error/    shared error types, result aliases, and context
  kurumi-containerd-helper/   target-aware Linux and Android syscall wrappers
  kurumi-containerd-host/     host resources and platform integration
  kurumi-containerd-runtime/  lifecycle, isolation, and state
  kurumi-containerd-tui/      ratatui manager and CLI action dispatch
```

The `kurumi-containerd` package produces the command-line binary. The library
crates provide internal implementation boundaries.

Dependencies flow in one direction: the CLI uses TUI, runtime, configuration,
and helper; TUI uses runtime and configuration; runtime uses host, configuration,
helper, and error; host uses
configuration, helper, and error; configuration uses error. Error and helper
do not depend on other workspace crates.

The error crate owns `ConfigError` and `RuntimeError`, preserving structured
variants and source chains. Configuration and runtime re-export their existing
error APIs. Host and runtime share `RuntimeError`, so policy callbacks cross
the crate boundary without error conversion. The helper retains its low-level
I/O errors.

## Configuration modules

The configuration crate keeps its public API in `lib.rs` and uses two
implementation modules. Related loading and validation helpers stay together
to keep navigation local:

```text
src/
  model.rs   serializable configuration schema and defaults
  config.rs  loading, path resolution, validation, and environment parsing
```

Callers import schema types from `kurumi_containerd_config`; the internal module layout
does not leak into the CLI or runtime.

## Lifecycle

A start operation validates configuration, locks the container identity,
prepares the root filesystem, allocates long-lived resources, and creates a
monitor. Each boot generation uses an intermediate process to create fresh PID
and optional network namespaces before forking init.

The monitor owns resources that must survive an internal container reboot:
rootfs mounts, console PTY, cgroup, and Android SELinux state where applicable.
It records init and monitor identities using the host boot ID, process start
time, and PID namespace inode so stale PIDs are not trusted.

Stop and restart use the same per-container lifecycle lock. Init signaling and
waiting use pidfds when available.

## Runtime modules

The runtime source is grouped by ownership:

```text
src/
  container/  behavior and policy applied inside the container
  runtime/    lifecycle orchestration, execution, state, and supervision
```

The main modules are:

- `runtime/lifecycle.rs`: start, monitor, boot generations, shutdown, and cleanup
- `runtime/boot.rs`: namespace-internal mounts, pivot, nested sandbox mounts, environment, and init execution
- `runtime/supervisor.rs`: wait/retry, signal policy, and child supervision helpers
- `runtime/state.rs`: public state types, secure persistence, metrics, and recovery
- `runtime/execute.rs`: namespace entry, command execution, and interactive login
- `container/environment.rs`: deterministic init and session environments
- `container/init.rs`: init-family detection and shutdown protocols
- `container/security.rs`: seccomp and protected kernel views

The host crate's `src/` contains:

- `process.rs`: pidfd handles and procfs process identity helpers
- `terminal.rs`: PTY allocation, descriptor passing, and console proxying
- `rootfs.rs` and `rootfs/install.rs`: rootfs preparation and archive installation
- `archive.rs`: private archive format detection and extraction
- `network.rs`: veth, bridges, NAT, forwarding, and rollback
- `cgroup.rs`: cgroup v1/v2 resource limits and hierarchy detection
- `android.rs`: Android host integration, exposed only on Android targets

Runtime supplies network state paths and shutdown callbacks to host; host does
not import lifecycle state or init-family policy.

## Nested user namespaces

When `container.security.allow_user_namespaces` is enabled, `runtime/boot.rs`
mounts `/run` before the regular container filesystem setup and creates two
additional filesystem instances after `pivot_root`:

- `/run/kurumi-containerd/proc`: writable procfs with `nosuid,nodev,noexec`
- `/run/kurumi-containerd/sys`: read-only sysfs with `ro,nosuid,nodev,noexec`

The normal `/proc` and `/sys` mounts continue to receive the configured
hardening and masks. The additional instances exist to satisfy the kernel's
visibility requirement when Docker, Podman, or another nested sandbox mounts
procfs/sysfs from an unprivileged user namespace. This option is explicitly
not a hostile-workload boundary because the pristine proc view can expose
host kernel sysctls.

## State and trust

Runtime directories and files are created with restrictive ownership and
permissions. State updates use no-follow opens, random temporary files, fsync,
and atomic rename. Recovery requires both a protected host record and matching
markers under `/run/kurumi-containerd` inside the container.

The runtime installs a classic-BPF seccomp filter, but container root retains
its capabilities and privileged container mechanisms still share the host
kernel. The design does not claim a hostile-workload security boundary.
