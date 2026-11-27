# 开发与验证

[简体中文](../zh/development.md) | [English](../en/development.md)

[文档目录](README.md)

从仓库根目录执行下列命令。使用当前 stable Rust，原生构建还需要 C 工具链和
依赖所需的系统库；跨平台工具链以 [CI](../../.github/workflows/ci.yml) 为准。
crate 职责与修改边界见[运行时架构](architecture.md)及 [AGENTS.md](../../AGENTS.md)。

## 常规检查

单元测试不需要 root。完整本地检查为：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

缩小测试范围可使用 `cargo test -p kurumi-containerd-config`，或在包名后追加
测试名称子串。不要在 Cargo 命令中添加 `--locked`。

## Feature 与构建

CLI 包名为 `kurumi-containerd`，默认不启用可选 feature：

| 构建选项 | 行为 |
| --- | --- |
| 不带 feature | CLI 和默认隔离模式 |
| `--features tui` | 加入终端管理界面和 `tui` 子命令 |
| `--features servicectl` | 加入共享 PID/IPC 配置与运行时支持 |
| `--features tui,servicectl` | 同时启用两项 |

```sh
cargo build --release -p kurumi-containerd --features tui,servicectl
cargo test --workspace --features tui,servicectl
cargo clippy --workspace --all-targets --features tui,servicectl -- -D warnings
```

`--workspace` 会构建独立 TUI crate，但不等于为 CLI 启用 `tui`。CI 的 release
产物启用 `servicectl`，debug 产物不启用；Linux 另有 `-with-tui` 产物。
Cargo profile 不会自动启用 feature。修改受 feature 控制的代码时，检查对应组合。

## 跨平台编译

CI 同时构建 debug 和 release。Linux 对应命令如下；debug 去掉 `--release`，
并按 CI 去掉 release 专用的 feature：

```sh
cargo build --workspace --release --features servicectl --target x86_64-unknown-linux-gnu
```

修改 helper 时，必须验证受影响的 Linux target 和 Android ABI，不能用宿主构建代替。
Linux 本地矩阵包括 x86_64/aarch64 GNU、armv7 GNU hard-float，以及 x86_64/aarch64
musl。本地无需验证 riscv64；CI 配置保持不变。
Android 需要 NDK 和 cargo-ndk，ABI 为 `arm64-v8a`、`armeabi-v7a`、`x86_64`、`x86`：

```sh
cargo ndk --platform 21 --target arm64-v8a build --workspace --release --features servicectl
```

对其余受影响 ABI 重复构建。实际矩阵和工具链安装步骤以 CI 为准。

## 特权集成验证

这些脚本需要可用的 Linux namespace、挂载权限、root 和相应 rootfs 或静态 C 编译器。
先构建，再检查主机：

```sh
cargo build --release -p kurumi-containerd --features servicectl
sudo target/release/kurumi-containerd check
sudo python3 tests/p1_runtime.py target/release/kurumi-containerd
sudo python3 tests/shared_isolation.py target/release/kurumi-containerd
```

`p1_runtime.py` 检查 credential 挂载传播、控制台、1 MiB 后台 PTY 输出和 cgroup v2
限制；需要 Python 3、可静态链接的 `cc`，以及可写 cgroup 层级。缺少 controller 时
报告 SKIP。生命周期脚本另需匹配主机架构的 LXC rootfs：

```sh
sudo python3 tests/container_lifecycle.py target/release/kurumi-containerd ./rootfs.tar.xz systemd
# Alpine 使用 openrc 替换 systemd。
```

这些验证独立于 `cargo test`。共享模式的真实服务栈、旧内核、禁用 PID/IPC namespace
的内核和 Android 验证要求见[共享模式服务](servicectl.md#privileged-runtime-acceptance)。
SKIP 表示未验证该项，不能记为通过。

## 基准与文档验证

无需 root 的基准：

```sh
cargo bench -p kurumi-containerd-host --bench archive
cargo bench -p kurumi-containerd-config --bench config
```

结果位于 `target/criterion/`，比较前后版本时保持同一主机和工具链。
归档基准覆盖生产 Rootfs 安装/导出接口；配置基准覆盖解析、加载和路径解析。
它们不测量容器启动。共享/独立模式的特权对比可使用
`tests/shared_pid_ipc_bench.py`，详见[基准 workflow](../../.github/workflows/benchmarks.yml)。

纯文档修改应核对中英文主题、相对链接、章节锚点、JSON 语法和示例命令与源码的一致性。
文档中的操作步骤本身不代表已在对应内核或设备上执行验证。
