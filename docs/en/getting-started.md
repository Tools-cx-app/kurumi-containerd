# Getting started

[简体中文](../zh/getting-started.md) | [English](../en/getting-started.md)

[Documentation](README.md)

This walkthrough uses Linux, a directory rootfs and the default `isolated` mode.
Prepare a trusted Linux rootfs archive matching the host architecture, with files
such as `sbin/init` directly at its top level. Installation accepts local archives;
verify their provenance, signatures or checksums separately. Android module paths
and startup are covered in [Android deployment](android-module.md).

## 1. Build and check the host

Build from the repository root with a current stable Rust toolchain. The workspace
uses the Rust 2024 edition; consult Cargo diagnostics and the CI toolchain for
the compiler requirements of the selected dependencies.

```sh
cargo build --release -p kurumi-containerd
sudo ./target/release/kurumi-containerd check
```

`check` reads neither HOME nor configuration. It reports namespace, mount, cgroup
and networking capabilities. Startup also validates the selected configuration's
requirements. Add `--features tui` to include the terminal manager, or
`--features servicectl` for shared PID/IPC mode. Both features default to off;
`--release` does not enable either automatically.

## 2. Install the first rootfs

This command assumes the HOME registry has no entry named `debian` yet:

```sh
sudo -H ./target/release/kurumi-containerd --name debian install ./rootfs.tar.zst
```

Installation creates or extends `$HOME/.kurumi-containerd/config.json`. A new
entry defaults to `$HOME/.kurumi-containerd/debian.rootfs`, using root's HOME in
this example. Existing entries retain their settings. Replacing an existing
rootfs requires explicit `--force`. Configuration is saved before extraction,
so a failure leaves it available for editing and retry; invalid JSON is never
overwritten.

To choose a custom path, edit root's registry before installation. A minimal
configuration is:

```json
[
  {
    "runtime": {},
    "container": {
      "name": "debian",
      "rootfs": "/srv/kurumi/debian/rootfs"
    }
  }
]
```

Configure exactly one of `rootfs` and `rootfs_image`. For a new sparse ext4 image,
use `rootfs_image` and pass `--size 8G` to `install`; directory targets reject
`--size`. When installation creates a new entry, `--size` selects the default
`debian.img` path instead. See [Installation](usage.md#install-a-local-rootfs)
and the [Configuration reference](configuration.md).

## 3. Start, verify and stop

```sh
sudo -H ./target/release/kurumi-containerd --name debian start
sudo -H ./target/release/kurumi-containerd --name debian info
sudo -H ./target/release/kurumi-containerd --name debian run uname -a
sudo -H ./target/release/kurumi-containerd --name debian enter
```

Exiting the interactive shell leaves the container running. Stop it when needed:

```sh
sudo -H ./target/release/kurumi-containerd --name debian stop
```

`start --foreground` connects the console to the current terminal; press Escape
followed by Ctrl-Q to request shutdown. A registry with one entry allows omission
of `--name`; multiple entries require selection. `exec` is an alias for `run`.
For pipes or redirection, explicitly invoke `run sh -c '...'`.

## 4. Back up and configure further

After stopping, export the rootfs to a new file in an existing output directory:

```sh
sudo -H ./target/release/kurumi-containerd backup debian ./debian.zst
```

Backup selects the container by its positional name; do not combine it with
`--name`. It excludes `config.json`, runtime state and bind mount sources. Read
[Backup and restore](usage.md#back-up-a-container) for format and restoration
limits. Configure networking, resource limits and mounts through the
[Configuration reference](configuration.md). For failures, use
[Troubleshooting](troubleshooting.md) to check configuration, host capabilities
and the full error chain.
