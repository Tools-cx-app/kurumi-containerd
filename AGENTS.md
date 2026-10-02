# Repository Guide

## Workspace boundaries

- `KurumiContainerd` is a privileged Linux/Android container runtime, not an Android app or a security sandbox. Runtime operations generally require root and a prepared rootfs; unit tests do not.
- The workspace has four packages: `kurumi-containerd` (`crates/kurumi-containerd-cli`, binary entrypoint), `kurumi-containerd-config` (strict TOML loading/validation), `kurumi-containerd-runtime` (lifecycle and isolation), and `kurumi-containerd-helper` (target-aware syscall wrappers).
- In `kurumi-containerd-runtime`, `runtime/` orchestrates lifecycle/state/exec, `host/` owns host resources, and `container/` applies in-container policy. Keep raw Linux/Android syscall wrappers in `kurumi-containerd-helper`; its crate-level unsafe/clippy allowances are intentional.

## Code ownership

- Put raw syscalls, libc/FFI details, file-descriptor primitives, and Linux/Android or architecture-specific wrappers in `kurumi-containerd-helper`. Keep policy, lifecycle decisions, and user-facing output out of this crate.
- Changes to `kurumi-containerd-helper` must preserve the target kernel ABI: use ABI-correct libc types, constants, structure layouts, and calling conventions under the appropriate target/architecture gates. Keep `unsafe` blocks minimal, document their safety invariants, validate pointers, lengths, ownership, and return values at the safe wrapper boundary, and do not expose an API as safe unless callers cannot violate those invariants.
- Put TOML schema, parsing, path resolution, defaults, and configuration validation in `kurumi-containerd-config`. It must not depend on runtime or host state.
- Put container lifecycle and isolation policy in `kurumi-containerd-runtime`: orchestration/state/exec in `runtime/`, host-owned resources in `host/`, and behavior applied inside the container in `container/`. Call `kurumi-containerd-helper` rather than duplicating unsafe syscall code.
- Put argument parsing, command dispatch, capability-report formatting, and other terminal-facing presentation in `kurumi-containerd-cli`. The package and binary are named `kurumi-containerd`; keep reusable runtime behavior out of the CLI.

## Verification

- Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` for a full local check.
- Run one package with `cargo test -p kurumi-containerd-config` or one test by substring, for example `cargo test -p kurumi-containerd-runtime configured_environment_overrides_session_environment`.
- CI does locked release cross-builds only. Match it with `cargo build --workspace --release --locked --target <linux-target>` or `cargo ndk --platform 21 --target <android-abi> build --workspace --release --locked`; Android ABIs are listed in `.github/workflows/ci.yml`.
- Only changes that touch `kurumi-containerd-helper` must preserve the ABI rules above and compile successfully for every affected Linux target and Android ABI in `.github/workflows/ci.yml`; changes outside that crate do not require ABI verification. A successful host-only build is not sufficient verification for helper changes.
- Unit tests exercise pure logic and lightweight host primitives. Lifecycle, namespace, mount, cgroup, networking, and Android integration need a suitable privileged host; use `sudo cargo run --release -p kurumi-containerd -- check` before manual runtime testing.
- For Linux cgroup/console regressions, build with `cargo build --release --locked -p kurumi-containerd`, then run `sudo python3 tests/p1_runtime.py target/release/kurumi-containerd`. The script creates temporary rootfs directories and requires Python 3, root, namespace/mount support, and a `cc` toolchain capable of static linking. It checks `/dev/console`, 1 MiB of background PTY output, and configured cgroup v2 limits. The cgroup case requires a writable hierarchy at `/sys/fs/cgroup` with `cpu`, `memory`, and `pids` available; missing controllers are reported as `SKIP`, not verified coverage. This script is separate from `cargo test` and CI.

## Runtime constraints

- Config is strict TOML. Relative host paths resolve from the config file, while init/mount/device paths are container paths. `Config::load_persistent` may atomically rewrite the source TOML to add a missing UUID; use `Config::load` in tests that must not mutate it.
- Changes to persisted state or recovery must preserve the trust model in `runtime/state.rs`: restrictive ownership/permissions, no-follow access, atomic replacement, and PID identity checks. A numeric PID alone is not trusted.
- Before configuring cgroup v2 limits, enable the requested controllers in both the hierarchy root and the `kurumi-containerd` parent's `cgroup.subtree_control`. Preserve already-enabled controllers and report unavailable controllers explicitly.
- Keep consuming PTY output while monitoring background containers, even when discarding it; an unread master can block init or services once its buffer fills. Bind `/dev/console` after mounting the container's `/dev` tmpfs and before detaching `/.old_root`, which supplies the host PTY path.
- Platform behavior is selected with `cfg(target_os = "android")` and architecture gates. A successful host build does not verify Android code; preserve and exercise the CI target matrix when changing gated code.
- Gate platform-specific modules, functions, imports, and exports with the narrowest applicable `#[cfg(...)]` or `cfg_if!`; do not expose unsupported functionality and reject it only at runtime. Keep matching tests under the same gate.
- The repository intentionally warns on unsafe code. Keep any unavoidable unsafe block narrowly allowed at the call site unless it belongs in `kurumi-containerd-helper`; do not weaken workspace lints.
