# Android root module

[[zh](android-module_CN.md) / [en](android-module.md)]

[Documentation](README.md)

## Requirements and installation

Install `kurumi-containerd-<version>-android-module.zip` through Magisk,
KernelSU or APatch Manager, then reboot. Android API 21 or newer and a working
`timeout` command are required. The ZIP contains `arm64-v8a`, `armeabi-v7a`,
`x86_64` and `x86` binaries; installation selects the device architecture.

The module provides the privileged CLI and boot script. Container operations
still require a kernel with the namespace, mount and resource features they use.
Run only trusted rootfs images and commands.

## Persistent files and commands

The module keeps persistent data outside its installation directory:

```text
/data/adb/kurumi-containerd/
├── .kurumi-containerd/config.json
├── autostart.txt
├── boot.log
└── tmp/
```

Create `config.json` with complete container entries using the
[configuration reference](configuration.md). The installer creates the private
directories and an initial `autostart.txt`, but does not generate a container
configuration or install a rootfs. Persistent directories must not be symlinks.

From a root shell, use the wrapper:

```sh
/data/adb/modules/kurumi-containerd/bin/kurumi-containerd check
/data/adb/modules/kurumi-containerd/bin/kurumi-containerd --name debian info
```

The wrapper sets `HOME=/data/adb/kurumi-containerd` and `TMP`/`TMPDIR` to its
`tmp` directory. `KURUMI_CONTAINERD_HOME` overrides this persistent base for
the wrapper and boot script. Use the same base when editing configuration.
The convenience `system/bin/kurumi-containerd` entry is available when the
root manager supports system mounting; otherwise use the full module path.

## Boot startup

Put one exact JSON `container.name` per line in `autostart.txt`. Blank lines
and lines starting with `#` are ignored:

```text
debian
alpine
```

Set `container.foreground` to `false` for every autostart entry. After
`sys.boot_completed=1`, the script processes entries in order, checks whether
each container is already running, and starts stopped containers in the
background. Each CLI attempt has a 120-second timeout. A failed entry is logged
and processing continues; there are no automatic retries.

Review `/data/adb/kurumi-containerd/boot.log` after boot failures. The boot
script redirects its output there when an autostart file is present; it is
not a persistent container-console log. See [CLI output](usage.md#output-and-diagnostics).

## Stop or disable startup

Stop a container from a root shell:

```sh
/data/adb/modules/kurumi-containerd/bin/kurumi-containerd --name debian stop
```

Removing its line from `autostart.txt` affects future boot attempts. Disabling
the module in the root manager disables boot startup; use `stop` explicitly
to stop a running container. Save `config.json` separately when backing up
rootfs data. Optional storage, GPU, Binder and socket integration is described
in the [configuration reference](configuration.md#android-integration).
