# Implementation status

[[zh](status_CN.md) / [en](status.md)]

[Documentation](README.md)

KurumiContainerd is an early Rust implementation for privileged Linux and Android
system containers. The core lifecycle is functional, but operators should evaluate
it on disposable systems before relying on it. It shares the host kernel and
expects trusted root filesystems and commands.

## Implemented

### Lifecycle and inspection

- Start, stop, and restart with foreground PTY consoles or background monitoring
- Init-aware graceful shutdown, timeout escalation, and internal reboot generations
- Interactive `enter` and non-interactive `run`, with command exit-status propagation
- Descriptor passing for consoles and interactive sessions
- Init PID, detailed info, live-container listing, host capability checks, and
  validated recovery scans
- Persistent UUIDs and process identities that resist PID reuse by checking host
  boot ID, process start time, and PID namespace inode

### Filesystems, installation, and backup

- Directory, ext4 image, btrfs image, and block-device rootfs modes, bind mounts,
  and volatile OverlayFS
- Local rootfs installation into a directory or a sparse ext4 image from tar,
  gzip tar, XZ tar, Zstandard tar, or ZIP; format detection uses the file header
- Staged installation with init validation before replacement; existing targets
  require `--force`, and the container must be stopped
- Rootfs backup for stopped containers through CLI and optional TUI, in `.zst`,
  `.xz`, `.zip`, or `.tar` format, with lifecycle locking during export
- Backup output streamed to a private `0600` temporary file and published only
  on success, without overwriting existing paths or symlinks

Backup exports rootfs contents rather than a raw disk image or a complete
container bundle. See [backup and restore](usage.md#back-up-a-container) for
selection rules, metadata preservation, exclusions, and restoration constraints.

### Isolation and host integration

- Mount, UTS, IPC, PID, and optional network namespaces
- Host, isolated, NAT, and existing-bridge networking
- TCP/UDP single-port forwarding with rollback and cleanup of owned rules
- Cgroup v1/v2 memory, CPU, and PID limits
- Execution in the running container's actual namespaces and cgroups, independent
  of configuration edits made after startup
- Classic-BPF seccomp filtering, read-only sys/proc controls, and bind masks
- Optional pristine proc/sysfs mounts for nested user-namespace sandboxes
- Optional Android storage, device, and socket integration in the runtime
- Magisk, KernelSU, and APatch root module with container boot startup; see the
  [Android module guide](android-module.md)

### Optional terminal manager

The `tui` Cargo feature is disabled by default. When enabled, the ratatui manager
loads the HOME JSON registry, displays configured entries including stopped
containers, refreshes runtime status, and dispatches operations through the same
CLI executable. It exposes install, backup, lifecycle actions, enter, run,
inspection, scan, and host checks. Builds without the feature have no `tui`
subcommand; workspace-wide builds still include the TUI crate.

## Current limitations

### Security and recovery

- Container root capabilities are not reduced. Seccomp and protected kernel views
  do not provide a hostile-workload security boundary.
- Enabling nested user namespaces adds a pristine writable proc view that can
  expose host kernel sysctls and reduces isolation. Use it only for trusted
  containers.
- Recovery requires the protected host recovery record and matching in-container
  identity markers. A discovered process or numeric PID alone is not trusted.
- Crash reconciliation is incomplete for some host-global resources.

### Host and interactive requirements

- Networking depends on host `ip` and `iptables`; port ranges and automatic
  upstream monitoring are not implemented.
- Interactive sessions cannot be detached and reattached.
- Interactive and foreground TUI actions require a supported graphical terminal
  on Linux (`xterm`, `kitty`, or `alacritty`). They are unavailable on headless
  Linux and Android; noninteractive TUI management remains available when built.
- TUI captured output is limited to the first 1 MiB of combined stdout/stderr.
  `run` takes separate executable/argument fields and does not parse shell syntax.
- Detached monitors redirect their standard streams to `/dev/null` after startup;
  this is not a persistent background-container logging facility.
- Namespace, mount, cgroup, and image operations require suitable privileges and
  kernel support. Image installation also needs loop devices and `mke2fs` or
  `mkfs.ext4`; image installation requires `--size`, which directory installation
  rejects.

### Archive and metadata constraints

- Installation accepts local archives only: there is no rootfs download or
  registry/URL installation. Archive contents must place init at the rootfs level,
  without an extra enclosing distribution directory.
- Backup requires root and a stopped container. It excludes config.json, runtime
  state, bind-mount sources, and discarded volatile OverlayFS changes. Save the
  configuration separately and avoid external writes during export.
- The backup output directory must already exist outside the rootfs. Mounted
  rootfs subtrees must be unmounted first; kernel filesystems and special files
  (devices, sockets, FIFOs) are rejected rather than skipped. Image sources need
  loop-device and mount support.
- Tar backups preserve ownership, permissions, timestamps, hardlinks, symlinks,
  and xattrs. ZIP backup loses ownership, timestamps, xattrs, special permission
  bits, and hardlink identity; it preserves ordinary permissions and safe
  relative symlinks, but rejects absolute/escaping symlinks and non-UTF-8 or
  ambiguous paths. ZIP installation cannot reliably preserve UID/GID or device
  nodes. Prefer tar or compressed tar for system rootfs archives.
- The current installer restores tar xattrs only on regular files. Its PAX reader
  cannot restore attribute values containing newlines. Directory/symlink xattrs
  and newline-containing values require an external tar implementation with
  xattr restoration enabled.

### Platform and interface maturity

- Android service supervision and full hardware/device policy are incomplete.
- There is no Android app, daemon/client API, general-purpose package installer,
  or stable machine-readable output contract. Local rootfs archive installation
  is available as described above.

Configuration fields are documented only when they have an active runtime path.
Treat failures as errors rather than assuming a degraded fallback. Consult the
[architecture](architecture.md) for ownership and trust rules and
[CLI usage](usage.md) for operational requirements.
