# Troubleshooting

[简体中文](../zh/troubleshooting.md) | [English](../en/troubleshooting.md)

[Documentation](README.md)

Keep the full error chain and enable `--verbose` for diagnostics. These Linux
examples assume the executable is on PATH:

```sh
sudo kurumi-containerd check
sudo -H kurumi-containerd --name debian --verbose info
```

Release diagnostics go to stderr; debug diagnostics go to stdout. Ordinary
results use stdout. After startup, detached monitors redirect standard streams
to `/dev/null`, so redirecting CLI stderr does not capture ongoing container
console logs. Use `start --foreground` to observe startup.

## Configuration and selection

| Symptom | Check and action |
| --- | --- |
| Missing configuration or name | Check the process HOME; `sudo -H` normally uses root's HOME. Match `container.name` exactly |
| Ambiguous selection | Put `--name NAME` before the subcommand. For `backup` and `export`, pass the source name as the first positional argument and omit `--name`. For `import`, `--name` sets the new container's name |
| Unknown field or invalid JSON | Use a JSON list of complete `runtime` and `container` objects, without comments, trailing commas or old TOML/pointer fields |
| Missing rootfs or init | Check the path base, archive layout and init path; use `install` to create a missing rootfs |
| A new configuration remains after failed install | Installation saves its default entry before extraction; edit the entry and retry |

Relative host paths resolve from the configuration directory, not the working
directory. Paths do not expand `~` or environment variables. See
[Configuration](configuration.md) for the complete rules.

## Startup and cgroups

`check` reads no configuration, so success does not prove every option is usable.
Startup still needs mount privileges, a suitable rootfs/init and the facilities
required by selected networking, security and resource options. Root inside a
restricted outer container may lack namespace or mount privileges.

For limits, verify controller availability and writable hierarchy access. Cgroup
v2 requires requested controllers in both the hierarchy root and the
`kurumi-containerd` parent's `cgroup.subtree_control`; preserve existing settings
while diagnosing. Shared mode always needs an exclusive writable cgroup v2
subtree and cgroup namespaces, even without limits. It does not require `cgroup.kill`.

`shared-pid-ipc` also requires a `servicectl` feature build and a foreground
supervisor. Normal systemd system instances are unsupported. Missing PID/IPC
namespaces never trigger automatic fallback. See [Shared-mode services](servicectl.md).

## Login, commands and consoles

- A shell exists but reports `No such file or directory`: check architecture,
  script interpreter and dynamic loader.
- Alpine login fails: try `enter --shell /bin/ash` or `enter --shell '/bin/busybox ash'`.
- A pipe is not interpreted: `run` passes arguments directly; invoke `run sh -c '...'`.
- No `tui` command: use a build with the `tui` feature. Interactive/foreground TUI
  actions also need Linux with `xterm`, `kitty` or `alacritty`. They are unavailable
  on headless systems and Android.
- To request shutdown from a foreground console, press Escape then Ctrl-Q.
  Exiting an `enter` shell leaves the container running.

## Installation and backup

Installation accepts local tar/compressed tar/ZIP archives. Directory targets
reject `--size`; image targets require it, along with loop, mount and ext4
formatting support. Existing targets require `--force`; verify selection and
paths before replacement.

Backup requires a stopped container and an existing output directory outside the
rootfs. The output file must not already exist. Unmount rootfs subtrees first;
special files and kernel filesystems cause rejection. ZIP loses metadata, and
tar restoration also has xattr limits. Save configuration and external mount
sources separately. See [Backup and restore](usage.md#back-up-a-container).

## Portable package import

Use `import` for a package created with `export`. Use `install` to restore a
rootfs archive created with `backup`. See [Migrate a container to another host](usage.md#portable-packages).

| Symptom | Check and action |
| --- | --- |
| Missing, unknown or duplicate mapping | Run `kurumi-containerd import ./debian.kurumi --inspect`. Supply exactly one `--map KEY=/absolute/path` for each listed key, using an existing path on the destination host. |
| `container 'debian' already exists` | Choose an unused destination name, for example `sudo -H kurumi-containerd --name debian-copy import ./debian.kurumi`, and include any required mappings. |
| Manifest or rootfs SHA-256 mismatch | Copy the package again from the trusted source, or export a fresh copy. Inspection checks the manifest checksum. Import also checks the compressed rootfs before installation. |

## Recovery and Android startup

`scan` requires protected host recovery records and matching in-container
identity; a PID alone does not authorize adopting a process. Shared recovery
also validates worker identity, mount namespace, generation and dedicated cgroup.
Avoid editing state records or deleting the runtime directory as routine repairs;
inspect process identity and the full error chain first.

Use the Android module wrapper to keep HOME/TMP consistent. Check
`/data/adb/kurumi-containerd/boot.log`, exact names in `autostart.txt` and
`foreground: false`. Each startup attempt has a 120-second timeout and no
automatic retry. See [Android deployment](android-module.md).
