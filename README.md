# KurumiContainerd

[[zh](README_CN.md) / [en](README.md)]

KurumiContainerd is a privileged Linux container runtime and command-line tool written
in Rust. It manages lightweight system containers with Linux namespaces,
mounts, cgroups, networking, and a persistent JSON configuration.

The name comes from **Kurumi**, the Japanese romanization of 胡桃
(“walnut”), combined with **containerd** to reflect the project's purpose.

This repository contains the CLI and runtime only. It does not contain an
Android application. The runtime can still expose optional Android host
resources when compiled for and executed on Android.

> KurumiContainerd is a container runtime, not a security sandbox. Run only root
> filesystems and commands you trust.

## Features

- Directory, ext4 image, btrfs image, and block-device root filesystems
- Local tar/ZIP rootfs installation into directories or sparse ext4 images
- CLI/TUI rootfs backup for stopped containers in `.zst`, `.xz`, `.zip`, or `.tar`
- Mount, PID, UTS, IPC, and optional network namespaces
- Host, isolated, NAT, and existing-bridge networking
- cgroup v1 and v2 memory, CPU, and process limits
- Bind mounts and volatile OverlayFS
- Background monitoring and foreground PTY consoles
- Container entry and command execution without an external `nsenter`
- Persistent, PID-reuse-resistant runtime state
- Seccomp filtering and read-only kernel views
- Optional user-namespace support for nested Docker, Podman, Flatpak, and Bubblewrap workloads
- Optional Android host integration without an Android app
- Magisk, KernelSU, and APatch module with container boot startup

## Requirements

- A Linux or Android kernel with the required namespace and mount features
- Root privileges
- Rust 1.85 or newer for the 2024 edition
- `ip` and `iptables` for NAT or bridge networking
- cgroup v1 or v2 when resource limits are configured
- A prepared Linux root filesystem containing an init executable

Run the host capability checks before starting a container:

```bash
sudo cargo run --release -p kurumi-containerd -- check
```

## Build

```bash
cargo build --release -p kurumi-containerd
```

The binary is written to `target/release/kurumi-containerd`.

The `tui` Cargo feature is disabled by default. To include the terminal manager:

```bash
cargo build --release -p kurumi-containerd --features tui
```

Without this feature, the `tui` subcommand is unavailable.
Select `-p kurumi-containerd` for a CLI-only build;
`--workspace` also builds the standalone TUI crate.

CI and releases provide `-with-tui` debug and release artifacts for every Linux
target. Android artifacts use the default build without TUI.

## Benchmarks

Run the benchmarks without root:

```bash
cargo bench -p kurumi-containerd-host --bench archive --locked
cargo bench -p kurumi-containerd-config --bench config --locked
```

The archive suite measures directory rootfs installation (format detection,
decompression, extraction and commit) for tar, gzip, xz, zstd and ZIP, plus backup
and compression for tar, xz, zstd and ZIP through the production `Rootfs` APIs.
It uses two deterministic workloads: 256 files of 4 KiB and four files of 1 MiB,
mixing text with pseudo-random bytes. Throughput counts uncompressed file bytes.
Archive generation and byte-for-byte round-trip checks happen outside the timed
loops. Each iteration uses a fresh destination; temporary directory setup and
cleanup are excluded. Production staging, metadata handling and backup `fsync`
remain timed. Slow archive cases use 10 samples.

Criterion measures JSON parsing, environment parsing (8, 64 and 512 entries),
configuration loading, and container path resolution with direct paths and symlinks.
Temporary rootfs and configuration fixtures are prepared outside the timed loops.
Filesystem benchmarks use warm caches and do not measure container startup or
image mounting. Results are saved under `target/criterion/` using Cargo's default
optimized bench profile.
The separate [Benchmarks workflow](.github/workflows/benchmarks.yml) runs these
two suites as parallel jobs and uploads `benchmark-results-archive` and
`benchmark-results-config` artifacts.
Each job also writes mean timings and throughput directly to the Actions Summary.

To compare changes on the same host:

```bash
cargo bench -p kurumi-containerd-host --bench archive --locked -- --save-baseline before
# Make the change, then compare against the saved baseline.
cargo bench -p kurumi-containerd-host --bench archive --locked -- --baseline before
```

## Releases

Push a version tag matching `[workspace.package].version` in `Cargo.toml`
(for example, `git tag v0.2.1 && git push origin v0.2.1`) to automatically
build and publish a GitHub Release. Tags with a prerelease suffix are marked
as prereleases. Publication waits for all ten build targets to succeed.

- GNU Linux (x86_64, aarch64, armv7, riscv64): `.deb`, `.rpm`, and `.tar.xz`.
- musl Linux (x86_64, aarch64) and Android (all four CI ABIs): `.tar.xz`.
- `SHA256SUMS` contains checksums for all release packages.

CI and tagged releases upload both debug and release builds for every target.
Release filenames are unchanged; debug filenames add `-debug` before the extension.
Debug builds write diagnostic logs to stdout; release builds use stderr.
Both Debian/RPM variants install the same package and executable paths; install one
variant at a time. The Android root module continues to use release binaries.
The CI combined artifact includes separate debug and release ZIPs.

To enable Telegram notifications for successful CI builds and published releases,
configure the repository Actions secrets `BOT_TOKEN` and `CHAT_ID`. After packaging, CI sends one message
with GitHub artifact links for all platform builds, the Android module and the
combined archives. After a GitHub Release is published, the release workflow sends
a release notification with the release page and direct download links for its
packages and `SHA256SUMS`.
Pull requests and runs without both secrets skip notifications.
Downloading CI artifacts requires GitHub sign-in and repository access; links expire
when the artifacts expire or are deleted. Public release assets can be downloaded
without signing in; private release assets require repository access.

Archives contain the executable, license, READMEs and example configuration.
Debian/RPM packages install the executable into `/usr/bin` and documentation
and the example configuration into `/usr/share/doc/kurumi-containerd`.
GNU packages require the libc version used by the build toolchain or newer;
use the musl archives when a portable Linux binary is needed.

## Quick Start

Create a configuration from the example and update the rootfs path:

```bash
sudo -H sh -c 'mkdir -p "$HOME/.kurumi-containerd" && exec vi "$HOME/.kurumi-containerd/config.json"'
```

Save all settings in this file, using [config.example.json](config.example.json) as a reference. Set your rootfs path:

```json
[
  {
    "runtime": {},
    "container": {"name": "debian", "rootfs": "/srv/kurumi/debian/rootfs"}
  }
]
```

```bash
sudo -H ./target/release/kurumi-containerd install ./rootfs.tar.zst
sudo -H ./target/release/kurumi-containerd start
sudo -H ./target/release/kurumi-containerd info
sudo -H ./target/release/kurumi-containerd enter
sudo -H ./target/release/kurumi-containerd stop
```

For interactive management of all registered containers, run
`sudo -H ./target/release/kurumi-containerd tui`. See [CLI usage](docs/usage.md)
for command details, configuration selection, and terminal requirements.

The JSON configuration format and path rules are documented in
[Configuration](docs/configuration.md). For migration from the removed
`-c/--config` entry points, see the [migration guide](docs/migration-config-pointer.md).

Back up a stopped container with
`sudo -H ./target/release/kurumi-containerd backup debian ./debian.zst`, or select
Backup in the TUI. The ID is the same `container.name` used by the runtime and TUI.
See [backup and restore](docs/usage.md#back-up-a-container) for metadata limits
and configuration files that must be saved separately.

## Commands

```text
install ARCHIVE [--size SIZE] [--force]
                        Install a local rootfs archive
backup CONTAINER_ID OUTPUT
                        Back up rootfs by its config.json name
start [--foreground]   Start the configured container
stop                   Gracefully stop it
restart [--foreground] Stop and start it
enter [USER]           Open an interactive login, defaulting to root
run COMMAND...          Run a non-interactive command
info                    Show live state and resource usage
pid                     Print the container init PID
show                    List containers in the configured work directory
scan                    Recover missing state for validated live containers
check                   Probe required host capabilities
```

## Documentation

Full documentation: [[zh](docs/README_CN.md) / [en](docs/README.md)].

| Guide | Language |
| --- | --- |
| CLI usage | [[zh](docs/usage_CN.md) / [en](docs/usage.md)] |
| Configuration reference | [[zh](docs/configuration_CN.md) / [en](docs/configuration.md)] |
| Runtime architecture | [[zh](docs/architecture_CN.md) / [en](docs/architecture.md)] |
| Implementation status | [[zh](docs/status_CN.md) / [en](docs/status.md)] |
| Android root module | [[zh](docs/android-module_CN.md) / [en](docs/android-module.md)] |
| Configuration migration | [[zh](docs/migration-config-pointer_CN.md) / [en](docs/migration-config-pointer.md)] |

## Acknowledgments

Special thanks to the following projects and their contributors:

- [Droidspaces](https://github.com/ravindu644/Droidspaces-OSS)
- [lxc](https://github.com/lxc/lxc)

## License

KurumiContainerd is licensed under the GNU General Public License v3.0 only
(`GPL-3.0-only`). See
[LICENSE](LICENSE).
