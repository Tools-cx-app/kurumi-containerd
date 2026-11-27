# 开发与验证

[简体中文](../zh/development.md) | [English](../en/development.md)

[文档目录](README.md)

从仓库根目录执行下列命令。使用当前 stable Rust，原生构建还需要 C 工具链和
依赖所需的系统库；跨平台工具链以 [CI](../../.github/workflows/ci.yml) 为准。
crate 职责与修改边界见[运行时架构](architecture.md)及 [AGENTS.md](../../AGENTS.md)。

## Rust 代码风格

根目录的 `rustfmt.toml` 固定使用 2024 格式风格、四空格缩进和 100 列行宽，
并由项目约定决定导入顺序。
源码、测试、基准及 xtask 使用相同约定；执行下列检查前先运行 `cargo fmt --all`。

- 导入依次分为标准库、第三方 crate、当前 crate 内部模块，组间空一行。
  第三方 crate 按名称排序。
- 同一作用域内合并来自同一 crate 的导入；可见性、属性或 `cfg` 条件不同的导入
  保持独立。每层先排 `self`，再排大写开头的名称、小写开头的名称，通配符放最后。
- 包含多个模块路径的外层导入组逐项换行，并保留尾逗号；短的内层分组保持紧凑，
  单一路径不加花括号，函数体仍正常格式化。
- 显式泛型参数的名称放在 `<...>` 中，约束统一放入 `where`，单个短约束也一样。
  `where` 独占一行，每项约束缩进四空格并保留尾逗号；带约束的 `impl` 参数沿用
  相同写法。保留原有约束，无约束生命周期和 const 参数不添加空的 `where`。
- 函数和方法的说明使用 `///`，放在属性之前，私有函数也遵循此规则；函数体内的
  实现说明使用 `//`。必要时说明失败条件，无需重复签名中的错误类型。

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
cargo xtask build --release --with-tui
cargo test --workspace --features tui,servicectl
cargo clippy --workspace --all-targets --features tui,servicectl -- -D warnings
```

`--workspace` 会构建独立 TUI crate，但不等于为 CLI 启用 `tui`。xtask 和 CI 的
release 构建启用 `servicectl`，debug 不启用；Linux 另有 `-with-tui` 产物。
直接使用 Cargo 时仍需显式选择 feature，Cargo profile 不会自动启用它们。
修改受 feature 控制的代码时，检查对应组合。

## 跨平台编译

CI 同时构建 debug 和 release。Linux 对应命令如下；debug 去掉 `--release`：

```sh
cargo xtask build --target x86_64-unknown-linux-gnu --release
```

修改 helper 时，必须验证受影响的 Linux target 和 Android ABI，不能用宿主构建代替。
Linux 本地矩阵包括 x86_64/aarch64 GNU、armv7 GNU hard-float，以及 x86_64/aarch64
musl。本地无需验证 riscv64；CI 配置保持不变。
Android 需要 NDK 和 cargo-ndk，ABI 为 `arm64-v8a`、`armeabi-v7a`、`x86_64`、`x86`：

```sh
cargo xtask build --target arm64-v8a --release
```

对其余受影响 ABI 重复构建。实际矩阵和工具链安装步骤以 CI 为准。

## 打包

`xtask` 是仅用于开发的 workspace 成员，运行时构建会排除它。
`dist` 先编译再打包，`package` 仅打包已有二进制：

```sh
cargo xtask dist --target x86_64-unknown-linux-gnu --release --with-tui
cargo xtask package --target x86_64-unknown-linux-gnu --release --with-tui
cargo xtask android-module --binaries artifacts --version-code 42
```

默认使用 workspace 版本，产物写入 `dist/`；可通过 `--output` 和 `--version` 修改。
`package --binary <路径>` 可指定 Cargo 目录外的二进制；未指定时通过 Cargo metadata
查找产物，也支持自定义 target 目录。所有相对路径从 workspace 根目录解析。

模块打包要求四个 ABI 的非空 release 文件都位于
`artifacts/kurumi-containerd-android-<abi>/kurumi-containerd`。
模块保留安装、启动脚本，并加入版本信息、许可证和示例配置。
`--version-code` 必须是正的 32 位整数。

GNU Linux 目标还会生成 Debian/RPM 包，需要 `readelf`、`dpkg-deb`、`rpmbuild` 和
`rpm`。依赖从目标 ELF 读取，不执行交叉编译的文件；musl、Android 仅生成 tar.xz。
文件名、debug/TUI 后缀及预发布版本排序沿用原发布规则。

普通 `cargo test --workspace` 包含归档、Android 模块及 shell 脚本测试。
安装原生打包工具后，额外验证 Debian/RPM：

```sh
cargo test -p xtask native_packages -- --ignored
```

交叉工具链可能设置 `CARGO_BUILD_TARGET`；调用 `cargo xtask` 前先取消该变量，
让任务程序在宿主机运行，再用 `--target` 选择运行时目标。CI 已显式处理。

## 特权集成验证

这些脚本需要可用的 Linux namespace、挂载权限、root 和相应 rootfs 或静态 C 编译器。
先构建，再检查主机：

```sh
cargo xtask build --release
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
