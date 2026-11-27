# Shared PID/IPC compatibility changes

The optional `servicectl` Cargo feature controls shared PID/IPC support and
is disabled by default. CI and tagged release builds enable it only for release
artifacts. Local release builds must pass `--features servicectl` explicitly.
Without it, `IsolationMode::SharedPidIpc` and shared runtime APIs are absent,
and shared configurations and state records cannot be loaded by that build.
Use a feature-enabled binary to manage an existing shared generation.

## Configuration and public structures

`container.isolation` is strict kebab-case JSON: `isolated` is the default and
`shared-pid-ipc` is explicit. Existing JSON keeps isolated behavior. Unknown
values fail validation; a failed namespace operation never triggers downgrade.
The example config spells out `isolated`.

The public Rust `ContainerConfig` gains `isolation: IsolationMode`. External
struct literals must supply this field or use a default update expression.
`ContainerState` gains the persisted actual mode and shared-generation identity
metadata (worker PID/start time, mount namespace inode and cgroup path).
`pid_namespace_inode` becomes `Option<u64>`; existing JSON numbers deserialize as
present values, and old records without a mode default to isolated. External
Rust state constructors and users of the inode field must update accordingly.
Shared records must include their mode-specific identity; absence of namespace
interfaces does not authorize recovery from an arbitrary PID.

CLI/TUI state output exposes actual isolation. Shared process/memory counts
come from the dedicated recursive cgroup subtree. The display is not a promise
of a stable machine-readable schema. Running-instance entry, inspection, stop
and recovery use persisted mode even after current JSON is changed.

## Runtime and checks

Shared mode requires exclusive writable cgroup v2 even with no limits; there
is no cgroup v1 shared backend. Controllers are needed only for requested limits.
`cgroup.kill` is optional; old kernels retain recursive member cleanup through
trusted process handles. Init and exec/login deny reboot syscalls. SIGHUP from a
shared supervisor is an exit, not an implicit namespace reboot. Restart must
empty and reap the old generation before starting the new one.

`check` remains independent of HOME/config, reports capabilities and per-mode
prerequisites, and bases its exit status on base runtime capabilities. Missing
PID/IPC namespaces alone do not invalidate the shared path. Network namespace
support is a non-host-network prerequisite. Startup still checks the actual
configuration, init compatibility, security facilities and cgroup delegation.

Shared mode is only for trusted services. It exposes host-visible PIDs and
shares SysV IPC/POSIX message queues. Private `/dev/shm`, cgroups and seccomp do
not create PID/IPC isolation. Normal systemd system instances and PID-1-dependent
inits are unsupported. Custom supervisors must remain foreground and avoid host
shutdown, global signals and shared IPC cleanup. Kurumi does not erase shared IPC
objects. Simultaneous SIGKILL of every supervising process, host shutdown and
kernel failure cannot have a user-space automatic-cleanup guarantee.

## Deployment and acceptance evidence

The [servicectl guide](servicectl.md) pins the inspected upstream commit and
provides rootfs assets, runtime prerequisites and exact commands. Dinit/s6/
servicectl remain external rootfs programs; no Go dependency enters Kurumi Rust.
Only implemented unit parser semantics apply, not full systemd isolation.

`tests/shared_isolation.py` is wired into the host integration workflow. It
prints PASS only after observing an assertion, and prints SKIP for unavailable
mount/cgroup prerequisites and environments it did not exercise. Its optional
VM filter probe uses invalid reboot magic; it never requests an actual reboot.

No executed deployment, disabled-PID/IPC kernel, old-kernel cleanup, target
Android or VM filter result is claimed by these notes. Record actual tool
results separately when running acceptance; source inspection and CI wiring
are not passing runtime evidence.
