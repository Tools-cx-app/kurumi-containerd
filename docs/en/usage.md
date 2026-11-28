# CLI usage

[简体中文](../zh/usage.md) | [English](../en/usage.md)

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
with `--name` is rejected. `check`, `show`, `scan`, and `daemon-reload` run independently of the configuration file.

The examples use `sudo -H`, selecting the registry under root's HOME. `install`
can create the registry and missing entries automatically. Changing HOME selects
a different registry. Most operations need root to create namespaces, mounts,
devices, cgroups, or network interfaces. See the [configuration
reference](configuration.md) for JSON and path rules and the [migration
guide](migration-config-pointer.md) for the removed configuration entry points.

```text
install ARCHIVE [--size SIZE] [--force]
backup CONTAINER_ID OUTPUT
start [--foreground]
stop
restart [--foreground]
enter [USER] [--shell COMMAND]
run COMMAND...          # alias: exec
info
pid
show
scan
daemon-reload
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
cargo build --release -p kurumi-containerd --features tui,servicectl
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
| Container services | Select a running container and press `s` to open its [service list](#manage-container-services). |

Status refreshes periodically. Narrow terminals stack the list and details;
very small terminals display a resize hint. To back up, select a stopped container,
choose Backup, and provide a new path ending in `.zst`, `.xz`, `.zip`, or `.tar`.
The selected `container.name` supplies the ID. `run` takes an executable and separate
argument fields, without interpreting shell quoting or pipes. Captured output is
limited to the first 1 MiB.
The Enter dialog has an optional Shell command field: use `/bin/ash` or
`/bin/busybox ash` for Alpine, or leave it empty to use the default login behavior.

On Linux, interactive and foreground commands open a separate graphical terminal.
Detection tries `xterm`, `kitty`, then `alacritty`. Set
`KURUMI_CONTAINERD_TERMINAL` to one of these executable names or its path to
override detection. The terminal stays open for reading the result; close it when
finished. These actions are unavailable on headless Linux and Android; other
noninteractive actions remain usable. An entry with `container.foreground: true`
also sends ordinary start/restart actions to the separate terminal.

### Manage container services

Select a running container and press `s`. The TUI uses the container's `run` command
to detect systemd or OpenRC and list each service's runtime and startup state. This
also works when BusyBox is PID 1 and OpenRC manages the services. The screen works
in a headless terminal with the `tui` feature alone. The container needs `/bin/sh`
and either `systemctl` or the OpenRC tools `rc-service`, `rc-status` and `rc-update`.

| Key | Action |
| --- | --- |
| ↑ / ↓ or `k` / `j` | Select a service. |
| Enter | Show the selected service's status. |
| `s` / `x` / `R` | Start, stop or restart the service. |
| `e` / `d` | Enable or disable startup. For OpenRC, this changes membership in the `default` runlevel. |
| `J` | Show the latest 100 journal entries for the selected systemd unit. Requires `journalctl` in the container. |
| PageUp / PageDown | Scroll command output. |
| `r` / Esc | Refresh the service list or return to the container list. |

Before applying a change, the TUI shows the service and container names. Press Enter
to confirm or Esc to cancel. After the command finishes, the TUI shows its exit
status and output and refreshes the service list.

## Install a local rootfs

`install` reads a local archive only: it does not download rootfs files or accept
URLs or registry sources. File headers, rather than extensions, identify tar,
gzip-compressed tar, XZ-compressed tar, Zstandard-compressed tar, or ZIP content.
For a directory target configured in the selected entry:

```bash
sudo -H kurumi-containerd install ./debian-rootfs.tar.zst
```

If the registry is missing or empty, or `--name NAME` selects a missing entry,
`install` first saves a default configuration. New entries use NAME, or `default`
when no name is supplied and the registry is empty. The default directory is
`$HOME/.kurumi-containerd/NAME.rootfs`; supplying `--size` instead selects a
`NAME.img` image. Existing entries retain their settings; multiple entries still
require `--name`, and invalid JSON is never overwritten. Configuration is saved
before extraction, so a failed installation leaves it available for editing or retry.

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

Lifecycle regression coverage includes systemd and OpenRC readiness, execution,
restart, graceful shutdown and cleanup. See [Development and verification](development.md)
for scripts, prerequisites and coverage requirements.

For kernels without PID/IPC namespaces, explicitly select
`container.isolation: "shared-pid-ipc"` and a foreground supervisor, as described
in [configuration](configuration.md#pid-and-ipc-isolation). A normal systemd
system instance is unsupported. The mode requires exclusive writable cgroup v2
but not `cgroup.kill`; it is for trusted workloads and never silently replaces a
failed isolated start. The deployable [Dinit/s6/servicectl guide](servicectl.md)
includes prerequisites, graceful rollback and exact service smoke commands.

Run the disposable shared-mode acceptance script separately:

```sh
sudo python3 tests/shared_isolation.py target/release/kurumi-containerd
```

Missing mount/UTS privileges or writable cgroup v2 produce explicit SKIP output.
This does not certify a kernel with PID/IPC namespaces disabled, Android, or the
old-kernel cgroup cleanup path unless that environment was actually used. No valid
reboot request is ever issued; an invalid-magic filter probe is disposable-VM-only
and opt-in. See the guide for the acknowledgement and command.


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

Commands join the running container's actual namespaces and cgroups. Its recorded
isolation mode controls entry; shared mode does not open/join absent PID/IPC
namespace interfaces. Editing isolation, network or resources in config.json
takes effect on the next start, not the running generation.
```bash
sudo -H kurumi-containerd enter
sudo -H kurumi-containerd enter developer
sudo -H kurumi-containerd --name alpine enter --shell /bin/ash
sudo -H kurumi-containerd --name alpine enter --shell '/bin/busybox ash'
sudo -H kurumi-containerd run uname -a
sudo -H kurumi-containerd run sh -c 'id && mount'
```

`enter` opens an interactive login as the specified user, defaulting to `root`.
`run` executes its remaining arguments directly. Invoke a shell explicitly for
pipes, redirection, or compound commands.

Use `enter --shell /bin/ash` to override the login shell, for example on Alpine.
The command starts with an absolute executable path inside the container and may
include arguments, such as `--shell '/bin/busybox ash'`. Quote the whole command
on the CLI; in the TUI Shell command field, enter `/bin/busybox ash` directly.
Shell-style quotes and backslashes group arguments; variables, substitutions and
pipes are not evaluated. Quote paths containing spaces within the command.
An explicit command bypasses `su` and the shell field in `/etc/passwd`; the selected
user's UID, GID, supplementary groups, home directory and login environment still
apply. `SHELL` is set to the executable path and `-l` is appended after the supplied
arguments, so BusyBox receives `busybox ash -l`.
Without `--shell`, `enter` tries the container's `su -l` first, then the account's
shell from `/etc/passwd`. The override applies only to this session and does not
modify `/etc/passwd` or config.json. A missing shell interpreter or dynamic loader
can still cause `No such file or directory` even when the shell file exists.
Container init and commands use the kernel `execve` syscall directly, preventing
host preload libraries such as `termux-exec` from rewriting container paths.
On Linux, `python3 tests/exec_interposition.py` verifies this with a preloaded
`execve` interposer; it requires Cargo and a host C compiler, without root.

## Inspect and recover

```bash
sudo -H kurumi-containerd info
sudo -H kurumi-containerd pid
sudo -H kurumi-containerd show
sudo -H kurumi-containerd scan
```

`info` prints container identity, actual isolation, init and monitor PIDs, init system, uptime, memory use, process count, generation, rootfs, and UUID. `pid` returns the init PID. Shared process/memory statistics cover its recursive dedicated cgroup, not host totals.

`show` lists containers tracked by daemon memory. `scan` validates those records
against process identities. Runtime state and recovery metadata remain in memory.
The CLI automatically starts the daemon when its socket cannot be connected,
printing startup logs. Linux uses `/var/kurumi/socket.sock` and Android uses
`/dev/kurumi/socket.sock`. Request logs append to `daemon.log` beside the socket.
The daemon detaches from the terminal and is adopted by PID 1. Daemon exit
terminates its containers and triggers resource cleanup. Run `sudo kurumi-containerd daemon-reload`
to restart it with an empty registry, or start it if absent. The command waits until
the new daemon answers. JSON configuration and rootfs contents remain on disk.

## Host checks

```bash
sudo kurumi-containerd check
```

`check` does not read HOME or configuration. It reports namespace capabilities,
OverlayFS, cgroup v2, pidfd, `ip` and `iptables`, distinguishing isolated and
shared-mode prerequisites. Shared mode additionally requires cgroup namespace
creation and exclusive writable cgroup v2; `start` validates the selected mode.
Missing PID/IPC namespaces do not alone prevent shared mode. Network namespaces
are required only for non-host networking. `start` validates the selected mode,
rootfs/init, security facilities and cgroup delegation rather than treating a
successful generic host check as authorization for every configuration.
