# Development and verification

[简体中文](../zh/development.md) | [English](../en/development.md)

[Documentation](README.md)

Run these commands from the repository root with current stable Rust. Native
builds also need a C toolchain and system libraries required by dependencies.
Use [CI](../../.github/workflows/ci.yml) for cross-toolchain setup. See
[Architecture](architecture.md) and [AGENTS.md](../../AGENTS.md) for crate ownership.

## Rust style

The root `rustfmt.toml` selects the 2024 formatting style, four-space indentation
and a 100-column limit. It leaves import ordering to the project conventions
below. Use the same conventions in source, tests, benchmarks and xtask. Run
`cargo fmt --all` before the checks below.

- Group imports in this order: standard library, third-party crates, then local
  modules, with a blank line between groups. Order third-party crates by name.
- Merge imports from the same crate within a scope, keeping different visibility,
  attributes and `cfg` conditions separate. Keep `self` first, uppercase-leading
  names before lowercase-leading names, and globs last within each group.
- Expand outer import groups containing several module paths, with one child per
  line and trailing commas. Keep short nested groups compact and single paths
  unwrapped. Function bodies still use normal formatting.
- Keep explicit generic parameter names in `<...>` and put their bounds in a
  `where` clause, including a single short bound. Put `where` on its own line,
  indent each predicate by four spaces, and retain trailing commas. Apply the
  same convention to bounded `impl` parameters. Preserve existing constraints;
  unconstrained lifetimes and const parameters do not need an empty clause.
- Use `///` before attributes for descriptions of functions and methods, including
  private functions. Use `//` for implementation notes inside bodies. Document
  failure conditions when helpful without repeating error types from signatures.

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
cargo xtask build --release --with-tui
cargo test --workspace --features tui,servicectl
cargo clippy --workspace --all-targets --features tui,servicectl -- -D warnings
```

`--workspace` builds the standalone TUI crate without enabling the CLI's `tui`
feature. xtask and CI release builds enable `servicectl`; debug builds omit it.
Linux also provides `-with-tui` artifacts. Direct Cargo builds must select features
explicitly; Cargo profiles do not enable them automatically. Verify the relevant
combinations when changing gated code.

## Cross-compilation

CI builds debug and release. Remove `--release` for a debug build:

```sh
cargo xtask build --target x86_64-unknown-linux-gnu --release
```

Helper changes require builds for every affected Linux target and Android ABI.
A host build alone is insufficient. The local Linux matrix includes x86_64 and
aarch64 GNU, armv7 GNU hard-float, and x86_64 and aarch64 musl. Local verification
may skip riscv64; leave its CI configuration unchanged.
Android needs the NDK and cargo-ndk. Its ABIs are `arm64-v8a`, `armeabi-v7a`,
`x86_64` and `x86`:

```sh
cargo xtask build --target arm64-v8a --release
```

Repeat for the other affected ABIs. CI is the source for the current matrix and
toolchain installation steps.

## Packaging

`xtask` is a development-only workspace member; runtime builds exclude it.
`dist` builds and packages, while `package` only packages an existing binary:

```sh
cargo xtask dist --target x86_64-unknown-linux-gnu --release --with-tui
cargo xtask package --target x86_64-unknown-linux-gnu --release --with-tui
cargo xtask android-module --binaries artifacts --version-code 42
```

Packages default to `dist/` and the workspace version. Use `--output` to change
the directory and `--version` to override the version. `package --binary <path>`
selects a binary outside Cargo's target directory. Otherwise the directory is
resolved through Cargo metadata, including custom target-directory settings.
All relative paths are resolved from the workspace root.

The module task requires all four nonempty release payloads at
`artifacts/kurumi-containerd-android-<abi>/kurumi-containerd`. It preserves module
scripts and adds version metadata, the license and the example configuration.
`--version-code` must be a positive 32-bit integer.

GNU Linux targets also generate Debian/RPM packages and need `readelf`,
`dpkg-deb`, `rpmbuild` and `rpm`. Dependency detection reads the target ELF without
executing it. musl and Android generate tar.xz only. Package names, debug/TUI
suffixes and prerelease version ordering match the release workflow.

Normal `cargo test --workspace` includes archive, Android module and shell-script
tests. To exercise Debian/RPM packaging with native package tools installed:

```sh
cargo test -p xtask native_packages -- --ignored
```

Cross-toolchain setup may set `CARGO_BUILD_TARGET`. Unset it before invoking
`cargo xtask` so the task binary runs on the host; select the runtime target with
`--target`. CI does this explicitly.

## Privileged integration

These scripts require a Linux host with namespaces, mount privileges, root and
suitable rootfs archives or a static C compiler. Build and check the host first:

```sh
cargo xtask build --release
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
