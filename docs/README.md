# Documentation

[[zh](README_CN.md) / [en](README.md)]

[Project overview](../README.md)

KurumiContainerd manages privileged Linux system containers on Linux and
Android. Start with the project quick start, then use the guides below for
operation, configuration and implementation details. Every guide has a complete
Chinese and English edition; the language links at the top switch the current
page without changing its topic.

| Guide | Contents | Language |
| --- | --- | --- |
| CLI usage | Container selection, TUI, installation, backup and restore, lifecycle, execution and diagnostics | [[zh](usage_CN.md) / [en](usage.md)] |
| Configuration reference | JSON layout, paths, field defaults, networking, resources, security, environment, mounts and Android options | [[zh](configuration_CN.md) / [en](configuration.md)] |
| Runtime architecture | Crate boundaries, lifecycle, host resources, state persistence and recovery trust | [[zh](architecture_CN.md) / [en](architecture.md)] |
| Implementation status | Implemented capabilities and current limitations | [[zh](status_CN.md) / [en](status.md)] |
| Android root module | Magisk, KernelSU and APatch installation, persistent files and boot startup | [[zh](android-module_CN.md) / [en](android-module.md)] |
| Configuration migration | Convert old TOML and name/file pointers to one JSON registry, validate and roll back | [[zh](migration-config-pointer_CN.md) / [en](migration-config-pointer.md)] |

For a new installation, configure an entry, check the host, install a local
rootfs, then start the container. For an existing installation using old
configuration entry points, stop the containers and follow the migration guide
first. Back up `config.json` separately from rootfs archives.

The runtime shares the host kernel and retains container root capabilities.
Run only trusted rootfs images and commands; the security options do not make
it a sandbox for hostile workloads.
