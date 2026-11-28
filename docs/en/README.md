# KurumiContainerd user guide

[简体中文](../zh/README.md) | [English](../en/README.md)

[Project overview](../../README.md) · [Language selection](../README.md)

KurumiContainerd manages privileged Linux system containers on Linux and Android.
Running a container requires root, a rootfs matching the host architecture, and
the relevant kernel facilities. Containers share the host kernel and retain root
capabilities; use trusted root filesystems and commands.

For a new installation, follow [Getting started](getting-started.md) to build,
check the host, install a rootfs and start a container. Existing installations
using TOML or `name/file` pointers should read [Configuration migration](migration-config-pointer.md)
first. Android root-module users can start with [Android deployment](android-module.md).

| Guide | Purpose |
| --- | --- |
| [Getting started](getting-started.md) | Build, install the first rootfs, verify operation and stop the container |
| [Host kernel requirements](kernel-features.md) | Kconfig options for startup, isolation, networking, storage and resource limits |
| [CLI and TUI](usage.md) | Container selection, lifecycle, command execution, backup, restore and systemd/OpenRC service management |
| [Portable container packages](usage.md#portable-packages) | Move a stopped container's rootfs and configuration to another host and map its external files |
| [Configuration reference](configuration.md) | JSON fields, defaults, paths, isolation, networking, limits and Android options |
| [Troubleshooting](troubleshooting.md) | Configuration, startup, cgroup, console, archive and recovery failures |
| [Android deployment](android-module.md) | Magisk, KernelSU and APatch modules, persistent files and boot startup |
| [Shared-mode services](servicectl.md) | Dinit, s6-rc and servicectl prerequisites, installation, startup, rollback and acceptance |
| [Configuration migration](migration-config-pointer.md) | Convert old TOML and pointer entries into one JSON registry |
| [Shared-mode compatibility](shared-isolation-release.md) | Features, configuration, Rust API and persisted-state compatibility |
| [Runtime architecture](architecture.md) | Seven crate boundaries, lifecycle, host resources and state trust |
| [Development and verification](development.md) | Local checks, feature builds, cross-compilation, privileged tests and benchmarks |
| [Implementation status and limits](status.md) | Available behavior, deployment requirements and remaining gaps |

Commands written as `kurumi-containerd` assume the executable is on PATH. For a
source build, substitute `./target/release/kurumi-containerd`. Linux examples use
`sudo -H`, so configuration is read from root's HOME. Save `config.json` and bind
mount sources separately when backing up a rootfs.
