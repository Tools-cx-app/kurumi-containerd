# Repository Guide

## Workspace boundaries

- `KurumiContainerd` is a privileged Linux/Android container runtime, not an Android app or a security sandbox. Runtime operations generally require root and a prepared rootfs; unit tests do not.
- The workspace has seven packages: `kurumi-containerd` (`crates/kurumi-containerd-cli`, binary entrypoint), `kurumi-containerd-tui` (terminal manager), `kurumi-containerd-config` (strict TOML loading/validation), `kurumi-containerd-error` (shared errors and context), `kurumi-containerd-host` (host resources), `kurumi-containerd-runtime` (lifecycle and isolation), and `kurumi-containerd-helper` (target-aware syscall wrappers).
- In `kurumi-containerd-runtime`, `runtime/` orchestrates lifecycle/state/exec and `container/` applies in-container policy. Host resources live in `kurumi-containerd-host`. Keep raw Linux/Android syscall wrappers in `kurumi-containerd-helper`; its crate-level unsafe/clippy allowances are intentional.

## Code ownership

- Put raw syscalls, libc/FFI details, file-descriptor primitives, and Linux/Android or architecture-specific wrappers in `kurumi-containerd-helper`. Keep policy, lifecycle decisions, and user-facing output out of this crate.
- Changes to `kurumi-containerd-helper` must preserve the target kernel ABI: use ABI-correct libc types, constants, structure layouts, and calling conventions under the appropriate target/architecture gates. Keep `unsafe` blocks minimal, document their safety invariants, validate pointers, lengths, ownership, and return values at the safe wrapper boundary, and do not expose an API as safe unless callers cannot violate those invariants.
- Put TOML schema, parsing, path resolution, defaults, and configuration validation in `kurumi-containerd-config`. It must not depend on runtime or host state.
- Put container lifecycle and isolation policy in `kurumi-containerd-runtime`: orchestration/state/exec in `runtime/` and behavior applied inside the container in `container/`. Put host-owned resources in `kurumi-containerd-host`; it must not depend on runtime. Call `kurumi-containerd-helper` rather than duplicating unsafe syscall code.
- Put shared `ConfigError`, `RuntimeError`, result aliases, context extensions, and error macros in `kurumi-containerd-error`. It must not depend on other workspace crates. Config and runtime re-export their existing error types for compatibility.
- Put argument parsing, command dispatch, capability-report formatting, and other terminal-facing presentation in `kurumi-containerd-cli`. The package and binary are named `kurumi-containerd`; keep reusable runtime behavior out of the CLI.
- Put the ratatui screen, TUI state, and TUI subprocess presentation in `kurumi-containerd-tui`; CLI owns the `tui` subcommand and passes its executable path to this crate.

## Module and test layout

- Use directory-based modules: `<module>/mod.rs`, declared with `mod <module>;` and the existing visibility and platform gates. Keep crate entrypoints as `src/lib.rs` or `src/main.rs`. This layout is compatible with Rust 2021; the workspace edition remains defined in `Cargo.toml`.
- Keep unit tests in each module's sibling `tests.rs`, loaded from `mod.rs` with `#[cfg(test)] mod tests;`. Crate-root unit tests live in `src/tests.rs`, declared from `lib.rs` or `main.rs`. Preserve test module paths, private-item access, and platform gates when moving tests.
- CLI argument-parsing and output unit tests may remain inline at the end of `src/main.rs` and `src/output/mod.rs` under `#[cfg(test)] mod tests { ... }`. Tests that launch the CLI binary remain integration tests in `crates/kurumi-containerd-cli/tests/`.
- Keep test-only helpers and imports in `tests.rs` where possible. Integration tests remain in each crate's `tests/` directory; privileged runtime scripts remain in the workspace `tests/` directory.

## Verification

- Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` for a full local check.
- Run one package with `cargo test -p kurumi-containerd-config` or one test by substring, for example `cargo test -p kurumi-containerd-runtime configured_environment_overrides_session_environment`.
- CI does locked release cross-builds only. Match it with `cargo build --workspace --release --locked --target <linux-target>` or `cargo ndk --platform 21 --target <android-abi> build --workspace --release --locked`; Android ABIs are listed in `.github/workflows/ci.yml`.
- Only changes that touch `kurumi-containerd-helper` must preserve the ABI rules above and compile successfully for every affected Linux target and Android ABI in `.github/workflows/ci.yml`; changes outside that crate do not require ABI verification. A successful host-only build is not sufficient verification for helper changes.
- Local verification does not require tests or cross-builds for `riscv64gc-unknown-linux-gnu`, including helper ABI verification. Skip this target; its CI configuration remains unchanged.
- Unit tests exercise pure logic and lightweight host primitives. Lifecycle, namespace, mount, cgroup, networking, and Android integration need a suitable privileged host; use `sudo cargo run --release -p kurumi-containerd -- check` before manual runtime testing.
- For Linux cgroup/console regressions, build with `cargo build --release --locked -p kurumi-containerd`, then run `sudo python3 tests/p1_runtime.py target/release/kurumi-containerd`. The script creates temporary rootfs directories and requires Python 3, root, namespace/mount support, and a `cc` toolchain capable of static linking. It checks systemd credential mount propagation without host leakage (using a minimal init, not actual systemd), `/dev/console`, 1 MiB of background PTY output, and configured cgroup v2 limits. The cgroup case requires a writable hierarchy at `/sys/fs/cgroup` with `cpu`, `memory`, and `pids` available; missing controllers are reported as `SKIP`, not verified coverage. This script is separate from `cargo test` and CI.

## Runtime constraints

- Config is strict TOML. Relative host paths resolve from the config file, while init/mount/device paths are container paths. `Config::load_persistent` may atomically rewrite the source TOML to add a missing UUID; use `Config::load` in tests that must not mutate it.
- Changes to persisted state or recovery must preserve the trust model in `runtime/state/mod.rs`: restrictive ownership/permissions, no-follow access, atomic replacement, and PID identity checks. A numeric PID alone is not trusted.
- Before configuring cgroup v2 limits, enable the requested controllers in both the hierarchy root and the `kurumi-containerd` parent's `cgroup.subtree_control`. Preserve already-enabled controllers and report unavailable controllers explicitly.
- Keep consuming PTY output while monitoring background containers, even when discarding it; an unread master can block init or services once its buffer fills. Bind `/dev/console` after mounting the container's `/dev` tmpfs and before detaching `/.old_root`, which supplies the host PTY path.
- Platform behavior is selected with `cfg(target_os = "android")` and architecture gates. A successful host build does not verify Android code; preserve and exercise the CI target matrix when changing gated code.
- Gate platform-specific modules, functions, imports, and exports with the narrowest applicable `#[cfg(...)]` or `cfg_if!`; do not expose unsupported functionality and reject it only at runtime. Keep matching tests under the same gate.
- The repository intentionally warns on unsafe code. Keep any unavoidable unsafe block narrowly allowed at the call site unless it belongs in `kurumi-containerd-helper`; do not weaken workspace lints.
