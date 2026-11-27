# Development and verification

[简体中文](../zh/development.md) | [English](../en/development.md)

[Documentation](README.md)

Run these commands from the repository root with current stable Rust. Native
builds also need a C toolchain and system libraries required by dependencies.
Use [CI](../../.github/workflows/ci.yml) for cross-toolchain setup. See
[Architecture](architecture.md) and [AGENTS.md](../../AGENTS.md) for crate ownership.

## Routine checks

Unit tests do not require root. The full local check is:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use `cargo test -p kurumi-containerd-config` for one package, or append a test-name
substring to narrow the run. Do not add `--locked` to Cargo commands.

## Features and builds

The CLI package is `kurumi-containerd`. Optional features default to off:

| Build option | Behavior |
| --- | --- |
| No features | CLI with default isolation |
| `--features tui` | Terminal manager and `tui` subcommand |
| `--features servicectl` | Shared PID/IPC configuration and runtime support |
| `--features tui,servicectl` | Both optional capabilities |

```sh
cargo build --release -p kurumi-containerd --features tui,servicectl
cargo test --workspace --features tui,servicectl
cargo clippy --workspace --all-targets --features tui,servicectl -- -D warnings
```

`--workspace` builds the standalone TUI crate without enabling the CLI's `tui`
feature. CI release artifacts enable `servicectl`; debug artifacts omit it.
Linux also provides `-with-tui` artifacts. Cargo profiles do not enable features
automatically. Verify the relevant combinations when changing gated code.

## Cross-compilation

CI builds debug and release. A Linux release command is shown below; remove
`--release` and the release-specific feature for the corresponding debug build:

```sh
cargo build --workspace --release --features servicectl --target x86_64-unknown-linux-gnu
```

Helper changes require builds for every affected Linux target and Android ABI.
A host build alone is insufficient. The local Linux matrix includes x86_64 and
aarch64 GNU, armv7 GNU hard-float, and x86_64 and aarch64 musl. Local verification
may skip riscv64; leave its CI configuration unchanged.
Android needs the NDK and cargo-ndk. Its ABIs are `arm64-v8a`, `armeabi-v7a`,
`x86_64` and `x86`:

```sh
cargo ndk --platform 21 --target arm64-v8a build --workspace --release --features servicectl
```

Repeat for the other affected ABIs. CI is the source for the current matrix and
toolchain installation steps.

## Privileged integration

These scripts require a Linux host with namespaces, mount privileges, root and
suitable rootfs archives or a static C compiler. Build and check the host first:

```sh
cargo build --release -p kurumi-containerd --features servicectl
sudo target/release/kurumi-containerd check
sudo python3 tests/p1_runtime.py target/release/kurumi-containerd
sudo python3 tests/shared_isolation.py target/release/kurumi-containerd
```

`p1_runtime.py` checks credential mount propagation, consoles, 1 MiB of background
PTY output and cgroup v2 limits. It needs Python 3, a `cc` toolchain supporting
static linking and a writable cgroup hierarchy. Missing controllers produce SKIP.
The lifecycle script additionally needs a matching-architecture LXC rootfs:

```sh
sudo python3 tests/container_lifecycle.py target/release/kurumi-containerd ./rootfs.tar.xz systemd
# Substitute openrc for systemd when using Alpine.
```

These checks are separate from `cargo test`. See [Shared-mode services](servicectl.md#privileged-runtime-acceptance)
for the real service stack, old kernels, disabled PID/IPC namespace kernels and
Android acceptance requirements. SKIP means unverified coverage.

## Benchmarks and documentation

Run benchmarks without root:

```sh
cargo bench -p kurumi-containerd-host --bench archive
cargo bench -p kurumi-containerd-config --bench config
```

Results are under `target/criterion/`. Keep the host and toolchain constant for
comparisons. Archive benchmarks use production Rootfs install/export APIs;
configuration benchmarks cover parsing, loading and path resolution. They do not
measure container startup. The privileged shared/isolated comparison uses
`tests/shared_pid_ipc_bench.py`; see the [benchmark workflow](../../.github/workflows/benchmarks.yml).

For documentation changes, check matching language topics, relative links,
section anchors, JSON syntax and agreement between commands and source code.
Published instructions alone are not evidence of execution on a kernel or device.
