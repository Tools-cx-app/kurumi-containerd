# Android root module

The Android module ZIP works with Magisk, KernelSU and APatch. It contains all
four Android ABIs and selects the device architecture during installation.
Android API 21 or newer is required.

## Install

Install `kurumi-containerd-<version>-android-module.zip` from the root manager,
then reboot. The module keeps persistent data in:

```text
/data/adb/kurumi-containerd/
├── .kurumi-containerd/config.json
├── autostart.txt
└── boot.log
```

Create the registry and point each entry at a TOML file. The same command is
available from a root shell:

```sh
export HOME=/data/adb/kurumi-containerd
/data/adb/modules/kurumi-containerd/bin/kurumi-containerd --name debian check
```

## Boot startup

Put one registry `name` per line in `autostart.txt`. Blank lines and lines
starting with `#` are ignored:

```text
debian
alpine
```

After `sys.boot_completed=1`, the module starts each selected container in
background mode. A running container is skipped, and an error is logged while
the next entry continues. Each attempt is limited to 120 seconds. Configure
each selected TOML with `container.foreground = false`. Review failures in
`/data/adb/kurumi-containerd/boot.log`.

The command wrapper sets `HOME` to the persistent directory. The
`system/bin` convenience entry is available when the root manager provides
system mounting; otherwise use the module path shown above.

To disable startup temporarily, disable the module in the root manager. To
stop one container, run `... --name NAME stop`; editing `autostart.txt` affects
the next boot.
