# Configuration

## JSON configuration

The CLI and TUI read all container settings directly from
`$HOME/.kurumi-containerd/config.json` using the running process's HOME.
HOME must be an absolute path. With `sudo -H`, use the target user's HOME,
normally root's.

Start from [config.example.json](../config.example.json), or this minimal file:

```json
[
  {"runtime": {}, "container": {"name": "debian", "rootfs": "/srv/kurumi/debian/rootfs"}}
]
```

The top level must be a nonempty list. Each entry contains `runtime` and
`container` objects. Unknown fields and invalid values are rejected.
`container.name` is the single name used for selection, TUI display, runtime
identity, and backup. Names must be unique with case-sensitive matching.
One entry is selected automatically; multiple entries require `--name NAME`
before the command. There is no outer `name` or `file` field or TOML indirection.

Relative host paths (`rootfs`, `rootfs_image`, mount sources, and
`environment_file`) resolve from the directory containing `config.json`.
Use absolute paths when migrating an existing configuration. Paths do not
expand `~` or environment variables. Init, mount targets, and device paths
are paths inside the container.

Missing or invalid configuration produces an error, without automatic creation
or fallback. `check` does not read HOME or configuration. Missing UUIDs are
written atomically to the selected JSON entry, preserving other entries, file
ownership, permissions, and a symlink to the configuration. Read-only TUI refresh
and backup do not persist UUIDs. See the [migration guide](migration-config-pointer.md).

The examples below show object fragments to merge into the selected entry.

## Runtime

```json
{
  "runtime": {
    "stop_timeout_seconds": 15
  }
}
```

Runtime-owned files are stored in `/run/kurumi-containerd` on Linux and
`$TMP/kurumi-containerd` on Android. The directory must be root-owned, must not
be group/world writable, and must not be a symlink.

## Container

```json
{
  "container": {
    "name": "debian-dev",
    "rootfs": "./rootfs",
    "hostname": "debian-dev",
    "init": "/sbin/init",
    "foreground": false,
    "volatile": false,
    "network": "host"
  }
}
```

Configure exactly one of `rootfs` or `rootfs_image`. A missing `uuid` is
generated on first use and atomically written back to the JSON file.
`volatile = true` places writable OverlayFS layers on tmpfs.

The `install` command creates a missing `rootfs` directory from a local
archive. When `rootfs_image` is configured, `install --size SIZE` creates a
sparse ext4 image at that path. Existing targets require `--force`.

Names may contain ASCII letters, digits, `.`, `_`, and `-`. The values
`.` and `..` are rejected.

## Networking

Network modes are `host`, `none`, `nat`, `gateway`, and `dhcp`.

```json
{
  "container": {
    "network_options": {
      "address": "172.28.0.2",
      "gateway": "172.28.0.1",
      "prefix": 16,
      "bridge": "kurumi-br0",
      "gateway_bridge": "",
      "dns": [
        "1.1.1.1",
        "8.8.8.8"
      ],
      "ports": [
        {
          "host": 8080,
          "container": 80,
          "protocol": "tcp"
        }
      ]
    }
  }
}
```

NAT and bridge setup currently use fixed host `ip` and `iptables`
executables. `gateway` attaches the container veth to the existing interface
named by `gateway_bridge`. It does not create or manage that bridge.
`dhcp` uses the same existing bridge and runs the first DHCP client found in
the container rootfs: `udhcpc`, `dhclient`, or `dhcpcd`.

```json
{
  "container": {
    "network": "dhcp",
    "network_options": {
      "gateway_bridge": "br0"
    }
  }
}
```

## Resources and security

```json
{
  "container": {
    "resources": {
      "memory_bytes": 536870912,
      "cpu_quota": 100000,
      "cpu_period": 100000,
      "pids": 512
    },
    "security": {
      "read_only_sys": true,
      "allow_user_namespaces": false
    }
  }
}
```

Resource limits use cgroup v2 when available and otherwise fall back to cgroup
v1. Memory must be at least 4 MiB. CPU quota and period must be configured
together; quota must be at least 1000. The PID limit must be between 1 and
4194304.

Set `allow_user_namespaces = true` for workloads that create unprivileged
user namespaces, such as Docker with userns-remap, Podman, Flatpak, or
Bubblewrap. In this mode the runtime keeps the normal `/proc` and `/sys`
hardening, and also exposes fresh filesystem instances at
`/run/kurumi-containerd/proc` and `/run/kurumi-containerd/sys` so nested
runtime mounts can pass the kernel visibility checks. The proc instance is
writable and the sysfs instance is read-only, so enabling this option weakens
container isolation. Enable it only for trusted workloads.

## Environment and mounts

```json
{
  "container": {
    "environment_file": "./container.env",
    "environment": {
      "LANG": "C.UTF-8"
    },
    "mounts": [
      {
        "source": "./shared",
        "target": "/mnt/shared",
        "read_only": false
      }
    ]
  }
}
```

Environment files accept `KEY=VALUE`, optional `export`, comments, quoted
values, empty values, and values containing `=`. Inline values win over file
values. Mount sources must exist, and targets must be absolute, non-root paths
without parent traversal.

## Android host integration

The optional `container.android` object controls selected storage, GPU,
Binder, Termux:X11, VirGL, PulseAudio, and SELinux integration when KurumiContainerd is
built for Android. These options fail closed on unsupported hosts. They are
runtime features and do not require or provide an Android application.
