# Dinit, s6-rc and servicectl in shared mode

[简体中文](../zh/servicectl.md) | [English](../en/servicectl.md)

[Documentation](README.md)

The assets in [examples/servicectl](../../examples/servicectl) provide a foreground
supervisor, a Dinit lifetime anchor, initialized s6-rc control plane, a confined
cgroup helper and a demo unit. They are for trusted rootfs workloads. Normal
systemd system instances are unsupported in `shared-pid-ipc` mode.

## Upstream contract and prerequisites

uses `/s6/rc`, `/run/s6/state`, and `/run/s6/compiled.servicectl`; its installer
alone does not start a live supervision tree. Its native Dinit client requires
control protocol **v5 or newer**. Use a Dinit build with native cgroup support.

The prepared rootfs must contain:

- `/usr/bin/python3` version 3.8 or newer, `/bin/sh`, `/usr/bin/env`, and the normal
  users/groups needed by your units (including root).
- Dinit with protocol >=5, native `run-in-cgroup` support, and `dinitctl`.
- s6, s6-rc and execline. Upstream requires `/bin/s6-rc`, `/bin/s6-rc-compile`,
  `/bin/s6-rc-update`, and `/usr/bin/execlineb`; on merged-usr distributions the
  corresponding symlinks suffice. `s6-svscan`, `s6-svscanctl`, and `s6-rc-init`
  must also be on PATH.
- servicectl plus `sys-notifyd`, `sys-cgroupd`, `sys-dbusd`, `sys-logd`,
  `sys-propertyd`, `sysvisiond`, and `sys-orchestrd` from the pinned source.
  The supplied installer builds these with Go >=1.22 on a build host targeting
  the rootfs architecture. Go is not needed in Kurumi or the deployed rootfs.

The host must provide root, mount/UTS/cgroup namespaces, procfs, seccomp, a
cgroup namespace, and a writable exclusive cgroup v2 subtree. `cgroup.kill` and
all resource controllers are not mandatory; only configured limits need
controllers. Kurumi creates an empty `kurumi-init` cgroup namespace root and
runs the supervisor in its `bootstrap` child. The rootfs must be stopped while
installing assets. Do not use this installer on the host root directory. Do not
bind host `/run`, D-Bus, Dinit, servicectl sockets, or the host cgroup root into
this rootfs. Optional D-Bus activation requires a separately configured private
bus; it is not needed for the demo. Absent journald/syslog may cause log
spill/degraded status; the example does not bind a host logging socket.

## Runtime build

Shared mode requires a Kurumi binary built with the optional `servicectl`
Cargo feature. CI and tagged release artifacts enable it in the release profile;
debug artifacts omit it. For a local build:

```sh
cargo build --release -p kurumi-containerd --features servicectl
```

The feature defaults to off. Cargo does not enable it automatically with
`--release`. Without the feature, configuration parsing rejects
`shared-pid-ipc` and shared runtime APIs are excluded from the build.

## Install

Prepare a matching-architecture Linux rootfs and install its Python/Dinit/s6/
s6-rc/execline packages first. Package availability differs by distribution;
the installer deliberately does not run a distribution package manager.
From the Kurumi repository on the build host:

```sh
git clone https://github.com/NEURAX-FX/servicectl.git /tmp/servicectl-src
git -C /tmp/servicectl-src checkout --detach 9bc7e99a4406a62b5dd745dac2f45472f6ed0b9a
sudo -E sh examples/servicectl/install.sh /srv/kurumi/debian/rootfs /tmp/servicectl-src
```

For cross compilation pass the appropriate Go target, for example
`sudo -E env GOOS=linux GOARCH=arm64 sh examples/servicectl/install.sh ROOTFS SOURCE`.
The source tree must be clean and at the pin. The installer copies only into the
chosen rootfs; it never executes the rootfs's init or mounts a host resource.

Merge this entry into the HOME registry used by `sudo -H`:

```json
[
  {
    "runtime": {"stop_timeout_seconds": 90},
    "container": {
      "name": "debian-services",
      "rootfs": "/srv/kurumi/debian/rootfs",
      "init": "/usr/local/sbin/kurumi-services",
      "isolation": "shared-pid-ipc",
      "network": "host"
    }
  }
]
```

The 90-second stop budget covers an in-flight startup command plus the example's
bounded rollback stages. Increase it for services with longer shutdown hooks.
Kurumi startup acknowledges execution of the entry, not application readiness;
wait for the ready marker.

## Smoke commands

These commands exercise the real rootfs service stack; they are instructions,
not recorded passing results:

An assert-based runner creates its own temporary HOME registry and exercises
the same real stack in an already prepared disposable rootfs:

```sh
sudo python3 examples/servicectl/smoke.py target/release/kurumi-containerd /srv/kurumi/debian/rootfs
```

It verifies service restart/stop hooks, persisted enablement, Kurumi restart and
normal shutdown. It may modify enablement and demo markers in the supplied rootfs.

```sh
sudo -H kurumi-containerd --name debian-services start
sudo -H kurumi-containerd --name debian-services run python3 -c \
  'import pathlib,time; p=pathlib.Path("/run/kurumi-services/ready"); end=time.monotonic()+60
while not p.exists():
 if time.monotonic()>end: raise SystemExit("bootstrap not ready")
 time.sleep(.1)'
sudo -H kurumi-containerd --name debian-services run servicectl start kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run servicectl is-active kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run servicectl status kurumi-demo.service --json
sudo -H kurumi-containerd --name debian-services run servicectl restart kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run servicectl stop kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run servicectl enable kurumi-demo.service
sudo -H kurumi-containerd --name debian-services restart
# Wait for /run/kurumi-services/ready again, then query the enabled unit.
sudo -H kurumi-containerd --name debian-services run servicectl is-active kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run cat /var/lib/kurumi-demo/stopped
sudo -H kurumi-containerd --name debian-services stop
```

`status` returns 3 for degraded/unknown aggregate health even if the workload is
running. Inspect its JSON diagnostics and use `is-active` for workload state; do
not turn a failed status into a passing health assertion. Demo `stopped` contains
the PIDs whose graceful hooks ran; inspect it in the stopped rootfs too.

## Startup, shutdown and failure contract

`kurumi-services` stays in the foreground as Kurumi's lifecycle anchor. It starts
Dinit with `--system --container`, explicit private `/run/dinitctl` and service
paths, and validates the native control connection. An internal Dinit anchor
keeps the manager alive when no application is active. The s6 `dinit` dependency
is an empty bundle because this entry already owns Dinit; a second manager must
not be started by the s6 graph.

The entry compiles the source graph, starts `s6-svscan`, runs `s6-rc-init`, brings
up the core daemons, then starts `default`, including the persisted enabled-unit
bundle. Only after those transitions succeed does it create
`/run/kurumi-services/ready`. A fresh private `/run` is required for each Kurumi
generation. Logs are `/run/kurumi-services/dinit.log`, `dinit-events.log` and
`control-plane.log`; all control sockets and runtime state stay in private `/run`.
Enablement/source definitions under `/s6/rc` and property state in the rootfs
survive stop/start. The demo markers are in `/var/lib/kurumi-demo`.

SIGTERM, SIGINT or SIGHUP requests orderly shutdown: stop enabled orchestrators
first to prevent restarts, request Dinit shutdown and wait for business-service
hooks, stop the remaining s6-rc services, then terminate/reap s6-svscan. The same
rollback runs after partial startup failure or either supervisor's unexpected
exit. The entry exits 0 only after a requested successful shutdown; startup,
manager-loss or cleanup errors produce exit 1. It records that result in
`/run/kurumi-services/stopped`. Kurumi then handles any surviving descendants by
exclusive cgroup ownership, including double-forked/new-session processes.
SIGKILL cannot execute hooks. Use Kurumi `restart` to replace a generation.

The monitor removes the cgroup after the worker has terminated and reaped its
descendants. `stop` waits for that cleanup instead of removing the same cgroup
again. If the monitor dies, the surviving worker removes the cgroup first, then
retires only state matching its PID, start time and host boot ID.

The cgroup helper wrapper always passes `--no-auto-mount` and a root beneath
`/sys/fs/cgroup/servicectl.slice`. The cgroup namespace root is the empty init
cgroup; `kurumi-services` and its services run in the `bootstrap` child. Thus
service-created cgroups stay below the delegated root and cannot reach siblings
or the host hierarchy. Upstream graph regeneration continues to invoke the
wrapper.

Only servicectl's implemented parser directives are supported. A `.service`
file does not provide full systemd semantics, `PrivateIPC=` isolation, a systemd
PID 1, or distribution-wide boot compatibility. Services/scripts must not invoke
host shutdown, `kill(-1)`, global `killall`, or shared IPC cleanup. SysV IPC and
POSIX message queues remain shared, and Kurumi does not remove their objects.

## Privileged runtime acceptance

```sh
cargo build --release -p kurumi-containerd --features servicectl
sudo python3 tests/shared_isolation.py target/release/kurumi-containerd
```

The script builds its own static mini supervisor and disposable rootfs, not a
mock runtime. It observes namespace sharing/absent interfaces, run exit statuses,
interactive enter, actual persisted mode, double fork/setsid descendants, nested
cgroups, restart, stop, monitor loss, init crash, failed boot, trusted scan, two
instances and an unaffected host sentinel. CI runs it on a normal host. Missing
mount privileges or writable cgroup v2 are printed as **SKIP**, not verified
coverage; absence of v2 is separately tested as an explicit refusal before init.

Only inside an explicitly disposable VM, an additional invalid-magic reboot
syscall probe can check seccomp denial. It never submits a valid reboot request:

```sh
sudo env KURUMI_DISPOSABLE_VM=1 python3 tests/shared_isolation.py \
  target/release/kurumi-containerd --vm-reboot-filter-probe
```

Acceptance without PID/IPC namespace support still requires a real VM/device
kernel with `CONFIG_PID_NS=n` and `CONFIG_IPC_NS=n`, with kernel-config evidence.
A normal kernel's shared branch cannot certify that environment. The old-kernel
cleanup path similarly requires a kernel without `cgroup.kill`; normal hosts
report that coverage separately. Android requires device-specific mount/UTS,
cgroup delegation, seccomp and SELinux validation. No unexecuted test is a pass.
