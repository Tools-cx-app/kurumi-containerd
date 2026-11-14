# CLI usage

[[zh](usage_CN.md) / [en](usage.md)]

[Documentation](README.md)

Use this guide to select a configured container, install its rootfs, manage its
lifecycle, and export or recover it. KurumiContainerd requires privileged host
operations; run only root filesystems and commands you trust.

## Invocation

```text
kurumi-containerd [--name NAME] [--verbose] COMMAND
```

The registry is `$HOME/.kurumi-containerd/config.json`: a nonempty JSON list of
complete configurations with unique `container.name` values. Most commands select
the only entry automatically. With multiple entries, put `--name NAME` before the
command; the name must match exactly, including case. `backup CONTAINER_ID OUTPUT`
is the exception: its positional ID selects `container.name`, and combining it
with `--name` is rejected. `check` alone does not load configuration.

The examples use `sudo -H`, so create the registry under root's HOME. Changing
HOME selects a different registry. Most operations need root to create namespaces,
mounts, devices, cgroups, or network interfaces. See the [configuration
reference](configuration.md) for JSON and path rules and the [migration
guide](migration-config-pointer.md) for the removed configuration entry points.

```text
install ARCHIVE [--size SIZE] [--force]
backup CONTAINER_ID OUTPUT
start [--foreground]
stop
restart [--foreground]
enter [USER]
run COMMAND...
info
pid
show
scan
check
tui
```

`tui` is available only in builds with the optional feature described below.

## Output and diagnostics

Results and success confirmations are plain stdout text without timestamps or
log levels. `pid` prints just the init PID and a newline:

```bash
pid=$(sudo -H kurumi-containerd pid)
```

Diagnostic logs use stdout in debug builds and stderr in release builds. INFO is
the default level; `--verbose` or `-v` enables DEBUG too. Logs use color only when
their stream is a terminal and `NO_COLOR` is unset. Failures include the chain of
error causes and return nonzero; `run` and `enter` preserve the executed command's
exit status. Closing a pipe that reads command results does not cause a panic.

Foreground start/restart reports completion when the container exits. Background
start/restart confirms startup with the init PID. After startup, detached monitors
redirect their standard streams to `/dev/null`; redirecting stderr therefore does
not provide persistent background-container logging. The TUI captures results and
diagnostics for noninteractive actions.

## Terminal manager

The `tui` Cargo feature is disabled by default. Enable it when building to include
the manager and its dependencies:

```bash
cargo build --release -p kurumi-containerd --features tui
sudo -H kurumi-containerd tui
```

Without that feature, the `tui` subcommand does not exist. The ratatui manager lists
all entries in the HOME registry, including stopped containers. Each action runs
the same CLI executable with the same HOME registry. Available actions are
install, backup, start/stop/restart (including foreground modes), enter, run,
info, pid, show, scan, and check. Check host remains available with an empty registry.

| Context | Keys and behavior |
| --- | --- |
| Container list | `j`/`k` or up/down selects a container. |
| Action selection | `h`/`l` or left/right chooses an action; Enter opens its dialog. Unavailable actions explain why. |
| Panes | `Tab`/`Shift+Tab` changes focus between containers, details, and output. PageUp/PageDown scrolls the focused details or output independently. Selecting another container resets details scrolling. |
| Dialog fields | `Tab`/`Shift+Tab` moves between fields; Tab at the last `run` argument adds another argument. Space toggles the selected install force field. |
| Submission | Enter submits. Stop, restart, scan, and forced install require a second Enter; confirmation identifies the target. Validation errors stay in the dialog. |
| Navigation | Escape cancels a dialog; `r` reloads the registry; `q` exits when no command is running. |

Status refreshes periodically. Narrow terminals stack the list and details;
very small terminals display a resize hint. To back up, select a stopped container,
choose Backup, and provide a new path ending in `.zst`, `.xz`, `.zip`, or `.tar`.
The selected `container.name` supplies the ID. `run` takes an executable and separate
argument fields, without interpreting shell quoting or pipes. Captured output is
limited to the first 1 MiB.

On Linux, interactive and foreground commands open a separate graphical terminal.
Detection tries `xterm`, `kitty`, then `alacritty`. Set
`KURUMI_CONTAINERD_TERMINAL` to one of these executable names or its path to
override detection. The terminal stays open for reading the result; close it when
finished. These actions are unavailable on headless Linux and Android; other
noninteractive actions remain usable. An entry with `container.foreground: true`
also sends ordinary start/restart actions to the separate terminal.

## Install a local rootfs

`install` reads a local archive only: it does not download rootfs files or accept
URLs or registry sources. File headers, rather than extensions, identify tar,
gzip-compressed tar, XZ-compressed tar, Zstandard-compressed tar, or ZIP content.
For a directory target configured in the selected entry:

```bash
sudo -H kurumi-containerd install ./debian-rootfs.tar.zst
```

For a new sparse ext4 image, configure `container.rootfs_image` and supply its
logical size. Binary suffixes include `M`, `G`, `MiB`, and `GiB`:

```bash
sudo -H kurumi-containerd install ./debian-rootfs.tar.zst --size 8G
```

The container must be stopped. Existing targets require `--force`; replacement is
staged and renamed only after extraction and init validation succeed. Image
installation also needs `mke2fs` or `mkfs.ext4`, loop devices, and mount privileges.
`--size` is mandatory for image targets and rejected for directory targets.

Archive entries go directly into the rootfs top level: `sbin/init` must not sit
inside an extra distribution directory. ZIP cannot reliably preserve UID/GID or
device nodes. Prefer tar for system rootfs archives, and read the restore metadata
limits in the next section before choosing a backup format.

## Back up a container

Stop the source before exporting it:

```bash
sudo -H kurumi-containerd --name debian stop
sudo -H kurumi-containerd backup debian ./debian.zst
```

`CONTAINER_ID` is the exact, case-sensitive JSON `container.name`, not a UUID.
It selects an entry even in a multi-entry registry without `--name`; supplying
`--name` with backup is an error. Backup changes neither the name nor the UUID.
The output suffix determines the archive format:

| Suffix | Contents |
| --- | --- |
| `.zst`, `.tar.zst` | Zstandard-compressed tar |
| `.xz`, `.tar.xz` | XZ-compressed tar |
| `.zip` | ZIP |
| `.tar` | Uncompressed tar |

Backup requires root and a stopped container. It exports the configured rootfs
directory or the filesystem inside an image, not the raw image. It excludes
config.json, runtime state, bind-mount sources, and discarded volatile OverlayFS
changes. Image sources need the loop-device and mount support required for
startup. A lifecycle lock prevents this container from starting during export;
also prevent external writes to the source.

Create the output directory in advance, outside the rootfs. Existing output paths,
including symlinks, are never overwritten. Export streams into a private `0600`
temporary file and publishes it only on success; ordinary failures remove the
partial file. Unmount rootfs subtrees first. Kernel filesystems and special files
(devices, sockets, and FIFOs) cause rejection rather than being silently omitted.

Tar and compressed tar preserve ownership, permissions, timestamps, hardlinks,
symlinks, and xattrs. ZIP preserves ordinary permissions and safe relative
symlinks, but loses ownership, timestamps, xattrs, special permission bits, and
hardlink identity; the command warns about this loss. ZIP rejects absolute or
escaping symlinks and non-UTF-8 or ambiguous paths instead of modifying them.
Prefer `.zst`, `.xz`, or `.tar` for system rootfs backups.

Restore by selecting the destination entry and using `install`:

```bash
sudo -H kurumi-containerd --name restored install ./debian.zst
```

Image destinations still require `--size`, and existing targets require `--force`.
The installer currently restores tar xattrs only on regular files; its PAX reader
cannot restore attribute values containing newlines. To restore directory or
symlink xattrs, or newline-containing values, use an external tar implementation
with xattr restoration enabled, such as GNU tar with
`--xattrs --xattrs-include='*'`. Save config.json separately: it is never packaged
by backup.

## Lifecycle

```bash
sudo -H kurumi-containerd start
sudo -H kurumi-containerd start --foreground
sudo -H kurumi-containerd restart
sudo -H kurumi-containerd stop
```

Background start detaches a monitor. Foreground start connects the container
console to the current terminal; press Escape followed by Ctrl-Q to request
shutdown. `restart` stops and starts the container and also accepts `--foreground`.

Stop chooses a shutdown protocol for the detected init family and escalates after
`runtime.stop_timeout_seconds`. A custom init receives `SIGTERM` for cleanup,
then `SIGKILL` if it has not exited by the timeout. Alpine with BusyBox PID 1
receives the BusyBox shutdown signal even when OpenRC manages services.

The privileged lifecycle CI in `.github/workflows/tests.yml` runs on Ubuntu using
Debian Bookworm (systemd) and Alpine 3.22 (OpenRC) rootfs archives from
`images.linuxcontainers.org`. Downloads are verified against the image index's
SHA-256. The CLI installs each archive; tests cover init readiness, command
execution and exit status, restart, graceful stop, process/state/cgroup cleanup,
mount isolation, and starting again after stop. An init-managed service records
startup and shutdown, so forced termination does not count as graceful stop.
Networking is `none`.

Use a matching host-architecture LXC rootfs to run the same integration script:

```bash
cargo build --release --locked -p kurumi-containerd
sudo target/release/kurumi-containerd check
sudo python3 tests/container_lifecycle.py target/release/kurumi-containerd ./rootfs.tar.xz systemd
# Use openrc instead of systemd for Alpine.
```

The script needs root, namespace/mount privileges, Python 3, and the archive.
It creates an isolated HOME registry and temporary rootfs, stops the container
on exit, and removes temporary files. LXC and Incus need not be installed.

## Nested sandboxing

Enable this option for workloads that create unprivileged user namespaces:

```json
{
  "container": {
    "security": {
      "allow_user_namespaces": true
    }
  }
}
```

Examples include Docker userns-remap, Podman, Flatpak, and Bubblewrap. The runtime
retains its normal `/proc` and `/sys` mounts and adds fresh views at
`/run/kurumi-containerd/proc` (writable) and `/run/kurumi-containerd/sys`
(read-only). The extra proc view can expose host kernel sysctls, reducing isolation.
Enable this option only for trusted containers.

## Enter and run

```bash
sudo -H kurumi-containerd enter
sudo -H kurumi-containerd enter developer
sudo -H kurumi-containerd run uname -a
sudo -H kurumi-containerd run sh -c 'id && mount'
```

`enter` opens an interactive login as the specified user, defaulting to `root`.
`run` executes its remaining arguments directly. Invoke a shell explicitly for
pipes, redirection, or compound commands.

Both join the running container's actual namespaces and cgroups. Changes to
network or resource settings in config.json take effect at the next start;
they do not alter isolation or limits for commands entering the current instance.

## Inspect and recover

```bash
sudo -H kurumi-containerd info
sudo -H kurumi-containerd pid
sudo -H kurumi-containerd show
sudo -H kurumi-containerd scan
```

`info` reports container identity, init and monitor PIDs, init system, uptime,
memory use, process count, generation, rootfs, and UUID. `pid` returns the init PID.
`show` reads live states from the configured work directory. To recover missing
state, `scan` searches procfs for namespace PID 1 processes. Recovery requires
agreement between the in-container identity and a root-owned recovery record.

## Host checks

```bash
sudo kurumi-containerd check
```

`check` needs no registry. It probes mount, PID, UTS, IPC, and network namespaces,
then reports OverlayFS, cgroup v2, pidfd, `ip`, and `iptables` availability. Failure
of a mandatory namespace probe returns an error.
