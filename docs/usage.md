# CLI usage

KurumiContainerd manages one configured container per invocation. Most commands
select an entry in `$HOME/.kurumi-containerd/config.json` using `--name NAME`.
Single-entry lists are selected automatically; multiple entries require an exact,
case-sensitive name before the command. `backup CONTAINER_ID OUTPUT` instead
selects the JSON `name` directly through its required positional container ID.

## Invocation

```text
kurumi-containerd [--name NAME] [--verbose] COMMAND
```

The process reads `$HOME/.kurumi-containerd/config.json`, containing a nonempty
list of entries with unique management `name` values and `file` paths pointing
to TOML files. See [configuration](configuration.md)
for the JSON format and the [migration guide](migration-config-pointer.md) for
upgrading existing commands. `check` is the only command that does not load a
configuration. The examples below use `sudo -H`: create the JSON under root's HOME.

Most operations require root because they create namespaces, mounts, devices,
cgroups, and network interfaces.

## Output and diagnostics

Command results and success confirmations go to stdout as plain text, without
timestamps or log levels. `pid` prints only the init PID and a newline, so it
can be captured directly:

```bash
pid=$(sudo -H kurumi-containerd pid)
```

Diagnostic logs are written to stdout in debug builds and stderr in release
builds. The default level is INFO; `--verbose` (or `-v`) also enables DEBUG.
Colors are enabled only when the selected logger stream is a terminal and
`NO_COLOR` is unset. Failed operations include their error cause chain and exit
with a nonzero status; `run` and `enter` preserve the command's exit status.
Closing a pipe reading CLI command results does not produce a panic.

Foreground start/restart reports completion after the container exits, while
background start/restart confirms startup with the init PID. Detached monitors
redirect their standard streams to `/dev/null` after startup; stderr redirection
is not a persistent background-container log facility. The TUI captures both
result and diagnostic streams for noninteractive actions.

## Terminal manager

```bash
sudo -H kurumi-containerd tui
```

The ratatui manager shows every entry in the HOME JSON registry, including
stopped containers. Use `j`/`k` or the arrow keys to select a container;
`h`/`l` or left/right to choose an action; Enter to open its dialog. Unavailable
actions show the reason. `Tab`/`Shift+Tab` switch focus between containers,
details and output; PageUp/PageDown scroll the focused details or output pane
independently. Switching containers resets the details position. Narrow terminals
stack the container list and details; very small terminals show a resize hint.

Inside a dialog, `Tab`/`Shift+Tab` move between input fields (`Tab` at the last
`run` argument adds another argument); `Space` toggles
force while the install force field is selected. Enter submits; stop, restart,
scan and forced install require a second Enter to confirm. Escape cancels the
dialog, `r` reloads the JSON registry,
and `q` exits when no command is in progress. Status refreshes periodically.
Validation errors appear in the dialog, and confirmation identifies the target.
Check host is also available when the registry is empty.

The manager exposes install, backup, start/stop/restart (including foreground modes),
enter, run, info, pid, show, scan and check. Each operation invokes the same
CLI executable and HOME registry. Select a stopped container, choose Backup,
and enter a new output path ending in `.zst`, `.xz`, `.zip` or `.tar`; the selected
JSON `name` supplies its container ID. `run` accepts an executable and separate
argument fields; it does not interpret shell quoting or pipes. Captured output
is limited to the first 1 MiB in the manager.

Interactive and foreground commands open a separate graphical terminal on
Linux. The manager detects `xterm`, `kitty` or `alacritty`, in that order;
set `KURUMI_CONTAINERD_TERMINAL` to one of those executable names or its
path to override detection. The terminal remains open after the command so
its result can be read; close it when finished. On headless Linux or Android,
these actions are unavailable, while noninteractive management remains in
the TUI. A TOML configuration with `container.foreground = true` also routes
ordinary start and restart through the separate terminal.

## Install a local rootfs

`install` accepts only a local archive. It never downloads a rootfs or accepts
a registry or URL source. Supported content formats are tar, gzip-compressed
tar, XZ-compressed tar, Zstandard-compressed tar, and ZIP. The format is
detected from the file header rather than its name or extension.

For a configured directory target:

```bash
sudo -H kurumi-containerd install ./debian-rootfs.tar.zst
```

For `container.rootfs_image`, provide the logical size of the new sparse ext4
image. First point JSON `file` to your image-target TOML. Binary suffixes such as
`M`, `G`, `MiB`, and `GiB` are accepted:

```bash
sudo -H kurumi-containerd install ./debian-rootfs.tar.zst --size 8G
```

An existing target is rejected unless `--force` is supplied. Replacement is
staged and renamed only after extraction and init validation complete. The
container must be stopped. Image installation additionally requires
`mke2fs` or `mkfs.ext4`, loop devices, and mount privileges.

Archive paths are installed directly at the rootfs top level, so `sbin/init`
must not be wrapped in an extra distribution directory. ZIP cannot preserve
UID/GID and device nodes reliably; tar is recommended for system rootfs
archives. `--size` is required for image targets and rejected for directory
targets.

## Back up a container

```bash
sudo -H kurumi-containerd --name debian stop
sudo -H kurumi-containerd backup debian ./debian.zst
```

`CONTAINER_ID` is the exact, case-sensitive `name` from the HOME JSON registry,
not the TOML `container.name`, UUID, or PID. This works with multiple registry
entries without `--name`; combining `--name` with backup is rejected. Neither
the TOML name nor its UUID is changed by backup.

The output suffix selects the format:

| Suffix | Contents |
| --- | --- |
| `.zst`, `.tar.zst` | Zstandard-compressed tar |
| `.xz`, `.tar.xz` | XZ-compressed tar |
| `.zip` | ZIP |
| `.tar` | Uncompressed tar |

Backup requires root and a stopped container. It exports the configured rootfs
directory or the filesystem inside its image, not a raw image, TOML, registry,
runtime state, bind-mount sources, or discarded volatile OverlayFS changes.
Image sources require the same loop-device and mount support as startup.
The lifecycle lock prevents starting this container during export; avoid external
writes to the source while backing up.

The output directory must already exist outside the rootfs. Existing paths,
including symlinks, are never overwritten. Archives are streamed to a private
temporary file with mode `0600` and published only after successful completion;
ordinary failures remove the partial file. Unmount any mounted rootfs subtrees
first. Kernel filesystems and special files (devices, sockets, FIFOs) are rejected
rather than silently skipped.

Tar and its compressed variants preserve ownership, permissions, timestamps,
hardlinks, symlinks, and extended attributes. ZIP preserves ordinary permissions
and safe relative symlinks but loses ownership, timestamps, xattrs, special
permission bits and hardlink identity; a warning is emitted. ZIP rejects absolute
or escaping symlinks and non-UTF-8 or ambiguous paths rather than changing them.
Prefer `.zst`, `.xz`, or `.tar` for system rootfs backups.

Restore rootfs contents through `install` after selecting the destination TOML:

```bash
sudo -H kurumi-containerd --name restored install ./debian.zst
```

Image targets still need `--size`; existing targets still need `--force`.
The current installer restores tar xattrs on regular files only, and its PAX
reader does not restore attribute values containing newlines. For directory or
symlink xattrs, or newline-containing values, use an external tar implementation
with xattr restoration enabled (for example GNU tar `--xattrs --xattrs-include='*'`).
Keep the TOML and registry separately if needed; backup does not package them.

## Lifecycle

```bash
sudo -H kurumi-containerd start
sudo -H kurumi-containerd start --foreground
sudo -H kurumi-containerd restart
sudo -H kurumi-containerd stop
```

Background start detaches a monitor process. Foreground start connects the
container console to the current terminal. In a foreground console,
`Escape` followed by `Ctrl-Q` requests shutdown.

Stop selects a shutdown protocol from the detected init family and escalates
after `runtime.stop_timeout_seconds`.

## Nested sandboxing

Set the following option when the container must run a workload that creates
unprivileged user namespaces:

```toml
[container.security]
allow_user_namespaces = true
```

This is useful for Docker userns-remap, Podman, Flatpak, and Bubblewrap. The
runtime keeps its regular `/proc` and `/sys` mounts and additionally provides
fresh mounts at `/run/kurumi-containerd/proc` and `/run/kurumi-containerd/sys`.
The first is writable and the second is read-only. Because the extra proc
view can expose host kernel sysctls, this option reduces isolation and should
be limited to trusted containers.

## Enter and run

```bash
sudo -H kurumi-containerd enter
sudo -H kurumi-containerd enter developer
sudo -H kurumi-containerd run uname -a
sudo -H kurumi-containerd run sh -c 'id && mount'
```

`enter` opens an interactive login and defaults to `root`. `run` executes
the remaining arguments directly; use a shell explicitly for pipes,
redirection, or compound commands.

## Inspect and recover

```bash
sudo -H kurumi-containerd info
sudo -H kurumi-containerd pid
sudo -H kurumi-containerd show
sudo -H kurumi-containerd scan
```

`show` reads live states from the configured work directory. `scan` searches
procfs for namespace PID 1 processes and reconstructs missing state only when
the in-container identity and root-owned recovery record agree.

`info` prints container identity, init and monitor PIDs, init system, uptime,
memory use, process count, generation, rootfs, and UUID.

## Host checks

```bash
sudo kurumi-containerd check
```

The check command probes mount, PID, UTS, IPC, and network namespaces, then
reports OverlayFS, cgroup v2, pidfd, `ip`, and `iptables` availability. A
failed mandatory namespace probe returns an error.
