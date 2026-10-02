# CLI usage

KurumiContainerd manages one configured container per invocation. To select a
different TOML, select its entry in `$HOME/.kurumi-containerd/config.json` using
`--name NAME`. Single-entry lists are selected automatically; multiple entries
require an exact, case-sensitive name before the command.

## Invocation

```text
kurumi-containerd [--name NAME] COMMAND
```

The process reads `$HOME/.kurumi-containerd/config.json`, containing a nonempty
list of entries with unique management `name` values and `file` paths pointing
to TOML files. See [configuration](configuration.md)
for the JSON format and the [migration guide](migration-config-pointer.md) for
upgrading existing commands. `check` is the only command that does not load a
configuration. The examples below use `sudo -H`: create the JSON under root's HOME.

Most operations require root because they create namespaces, mounts, devices,
cgroups, and network interfaces.

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
