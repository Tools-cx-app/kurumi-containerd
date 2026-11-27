# KurumiContainerd

[[zh](README_CN.md) / [en](README.md)]

KurumiContainerd 是一个使用 Rust 编写的特权 Linux 容器运行时和命令行工具。它通过
Linux namespace、挂载、cgroup、网络及持久化 JSON 配置管理轻量级系统容器。

项目名中的 **Kurumi** 是日语“胡桃”（くるみ）的罗马字，后缀
**Containerd** 表明它是一个容器运行时项目。

本仓库只包含 CLI 和运行时，不包含 Android App。运行时在 Android 目标上编译
和执行时，仍可按配置暴露部分 Android 主机资源。

> KurumiContainerd 是容器运行时，不是安全沙箱。请只运行可信的 rootfs 和命令。

## 主要能力

- 目录、ext4 镜像、btrfs 镜像和块设备 rootfs
- 从本地 tar/ZIP 归档安装目录 rootfs 或稀疏 ext4 镜像
- CLI/TUI 导出停止容器的 rootfs，支持 `.zst`、`.xz`、`.zip`、`.tar`
- Mount、PID、UTS、IPC 以及可选的 Network namespace
- 显式共享 PID/IPC 模式，供缺少对应 namespace 的内核运行可信服务
- Host、隔离、NAT 和已有网桥网络模式
- cgroup v1/v2 内存、CPU 和进程数限制
- Bind mount 和临时 OverlayFS
- 后台 monitor 与前台 PTY 控制台
- 无需外部 `nsenter` 的容器进入和命令执行
- 防 PID 复用的持久化运行状态
- seccomp 过滤和只读内核视图
- 可选的 user namespace 支持，用于容器内运行 Docker、Podman、Flatpak 和 Bubblewrap
- 不依赖 Android App 的可选 Android 主机集成
- Magisk、KernelSU、APatch 通用模块，支持容器开机启动

## 环境要求

- 支持所需 namespace 和挂载功能的 Linux 或 Android 内核
- root 权限
- Rust 1.85 或更高版本
- NAT/网桥网络所需的 `ip` 和 `iptables`
- 隔离模式资源限制需要 cgroup v1/v2；共享模式始终需要可写且专属的 cgroup v2 子树
- 包含 init 可执行文件的 Linux rootfs

启动容器前可检查主机能力：

```bash
sudo cargo run --release -p kurumi-containerd --features servicectl -- check
```

## 构建

本地与 CI 统一使用 `cargo xtask` 编译、打包：

```bash
cargo xtask build --release
cargo xtask build --release --with-tui
cargo xtask dist --target x86_64-unknown-linux-gnu --release
```

`build` 默认构建 debug；`--release` 同时启用支持共享 PID/IPC 的 `servicectl`。
`--with-tui` 为 Linux 构建加入终端管理界面。宿主构建的二进制位于
`target/release/kurumi-containerd`；`--target` 接受 Linux triple 或 Android
triple/ABI，产物位于 `target/<triple>/<profile>/`。

`dist` 编译后打包到 `dist/`；`package` 接受相同选项，仅打包已有二进制，
也可通过 `--binary <路径>` 指定文件。GNU Linux 生成 tar.xz、Debian 和 RPM 包，
musl、Android 生成 tar.xz。原生包需要 `readelf`、`dpkg-deb`、`rpmbuild` 和 `rpm`；
Android 构建需要 NDK 和 cargo-ndk，使用 API 21。

CI 和 Release 为所有 Linux target 提供带 `-with-tui` 后缀的 debug/release 产物，
Android 产物不含 TUI。模块打包及交叉编译示例见[开发与验证](docs/zh/development.md)，
全部命令见 `cargo xtask --help`。仍可直接用 Cargo 的 `--features tui,servicectl`
构建；两项 feature 均默认关闭，Cargo profile 本身不会启用它们。

## 基准测试

无需 root 即可运行基准测试：

```bash
cargo bench -p kurumi-containerd-host --bench archive
cargo bench -p kurumi-containerd-config --bench config
```

归档基准通过实际 `Rootfs` 接口测量 tar、gzip、xz、zstd、ZIP 的目录 rootfs 安装
（格式识别、解压、文件提取、提交），以及 tar、xz、zstd、ZIP 的备份压缩。
数据集分别为 256 个 4 KiB 文件和 4 个 1 MiB 文件，使用确定性的文本与伪随机字节混合，
吞吐量按未压缩的文件字节数计算。归档生成和逐字节往返校验在计时之外执行。
每轮使用全新的目标位置，临时目录准备和清理不计时；生产路径中的暂存、元数据处理
和备份 `fsync` 计入耗时。较慢的归档用例采用 10 个样本。

使用 Criterion 测量 JSON 解析、环境变量解析（8、64、512 项）、完整配置加载，
以及普通路径、绝对和相对符号链接的容器路径解析。临时 rootfs 和配置文件均在
计时循环之外准备。文件系统测试使用预热后的缓存，不测量容器启动或镜像挂载。
使用 Cargo 默认的优化 bench profile，结果保存在 `target/criterion/`。
独立的 [Benchmarks workflow](.github/workflows/benchmarks.yml) 以两个并行 job 运行，
分别上传 `benchmark-results-archive` 和 `benchmark-results-config` artifact。
各 job 还会将平均耗时和吞吐量直接写入 Actions Summary。

在同一台主机上对比修改前后的性能：

```bash
cargo bench -p kurumi-containerd-host --bench archive -- --save-baseline before
# 修改代码后，与保存的基线对比。
cargo bench -p kurumi-containerd-host --bench archive -- --baseline before
```
需要 root、静态 C 编译器和可写 cgroup v2 的 Linux 主机，直接对比原有独立 PID/IPC namespace
与 `shared-pid-ipc` 的容器启动至 supervisor 就绪时间及 `run` 往返时间：

```bash
sudo python3 tests/shared_pid_ipc_bench.py target/release/kurumi-containerd --iterations 20
```

输出每种模式各操作的中位数和 P95，耗时包含 CLI 进程开销；该 benchmark 使用相同的最小
静态 supervisor，不依赖发行版 rootfs。 [Benchmarks workflow](.github/workflows/benchmarks.yml)
在具备所需特权的 runner 上执行该对比。

## 自动发布

推送与 `Cargo.toml` 中 `[workspace.package].version` 一致的版本标签
（例如 `git tag v0.2.1 && git push origin v0.2.1`）后，自动构建并创建 GitHub Release。
带预发布后缀的版本标记为 prerelease，全部 10 个目标构建成功后才发布。

- GNU Linux（x86_64、aarch64、armv7、riscv64）：`.deb`、`.rpm`、`.tar.xz`。
- musl Linux（x86_64、aarch64）和 Android（CI 中的四种 ABI）：`.tar.xz`。
- `SHA256SUMS` 包含所有发布包的校验和。

CI 和标签发布均为每个目标上传 debug、release 两种构建。
release 文件名保持不变，debug 文件名在扩展名前追加 `-debug`。
debug 构建的诊断日志输出到 stdout，release 构建输出到 stderr。
两种 Debian/RPM 包使用相同的包名和安装路径，一次只安装其中一种。
Android root 模块继续使用 release 二进制。
CI 汇总 artifact 包含独立的 debug、release ZIP 包。

在仓库 Actions secrets 中配置 `BOT_TOKEN` 和 `CHAT_ID`，即可启用 CI 构建成功及
GitHub Release 发布后的 Telegram 通知。CI 打包完成后发送一条消息，包含所有平台
构建、Android 模块和汇总包的 GitHub artifact 下载链接。Release 发布成功后发送
发布通知，包含 Release 页面、发布包和 `SHA256SUMS` 的直接下载链接。
Pull Request 或未同时配置两个 secret 时跳过通知。
下载 CI artifact 需要登录 GitHub 并具有仓库访问权限；artifact 过期或被删除后，链接失效。
公开 Release 附件无需登录即可下载；私有仓库的 Release 附件仍需要仓库访问权限。

压缩包包含可执行文件、许可证、README 和示例配置。
Debian/RPM 包将程序安装到 `/usr/bin`，文档和示例配置安装到
`/usr/share/doc/kurumi-containerd`。GNU 包要求系统 libc 不低于构建工具链使用的版本；
需要便携 Linux 程序时可使用 musl 压缩包。

## 快速开始

```bash
sudo -H sh -c 'mkdir -p "$HOME/.kurumi-containerd" && exec vi "$HOME/.kurumi-containerd/config.json"'
```

参考 [config.example.json](config.example.json)，将所有配置写入这个文件，并修改 rootfs 路径：

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

使用 `sudo -H ./target/release/kurumi-containerd tui` 可打开 ratatui 管理界面，
浏览所有已登记容器并执行现有 CLI 命令。按键、配置选择和终端要求见
[CLI 使用](docs/zh/usage.md)。JSON 配置格式和路径规则见[配置参考](docs/zh/configuration.md)；
从已删除的 `-c/--config` 入口迁移时，参阅[配置入口迁移指南](docs/zh/migration-config-pointer.md)。

备份示例：`sudo -H ./target/release/kurumi-containerd backup debian ./debian.zst`。
先停止容器；`debian` 统一使用 JSON 中的 `container.name`，与运行时、TUI 一致。
TUI 选择容器后使用 Backup 输入输出路径；备份不包含 config.json，ZIP 有元数据限制，
详见[备份与恢复说明](docs/zh/usage.md#back-up-a-container)。

缺少 PID/IPC namespace 的内核可显式设置 `"isolation": "shared-pid-ipc"`，
使用前台服务 supervisor；不会在隔离启动失败后自动降级。该模式共享宿主可见 PID
和 IPC，仅供可信工作负载，必须有 cgroup v2，但不要求 `cgroup.kill`。不支持普通
systemd 系统实例。[Dinit + s6/servicectl 部署示例](docs/zh/servicectl.md)提供固定上游
提交、运行依赖、完整启动/回滚、最小业务 unit 和验收命令。Go 仅用于构建 rootfs
服务工具，不加入 Kurumi 的 Rust 依赖。运行代际持久化实际隔离模式，修改 JSON
不会改变现有实例的 run/enter/info/stop/scan 行为。`check` 不读取 HOME 或配置，
分项报告两种模式的能力要求；启动时仍检查选定模式的实际前提。


## 命令

```text
install ARCHIVE [--size SIZE] [--force]
                        从本地归档安装 rootfs
backup CONTAINER_ID OUTPUT
                        以 config.json 的 name 选择容器并备份 rootfs
start [--foreground]   启动配置中的容器
stop                   优雅停止容器
restart [--foreground] 重启容器
enter [USER] [--shell COMMAND]
                        进入交互式登录（默认 root），可指定 shell
run COMMAND...          执行非交互命令
info                    显示运行状态和资源使用情况
pid                     输出容器 init PID
show                    列出运行时目录中的容器
scan                    验证运行进程并恢复缺失状态
check                   检查主机能力
```

## 文档

从[快速开始](docs/zh/getting-started.md)开始，或查阅完整文档：[[zh](docs/zh/README.md) / [en](docs/en/README.md)]。

| 指南 | 语言 |
| --- | --- |
| CLI 使用 | [[zh](docs/zh/usage.md) / [en](docs/en/usage.md)] |
| 配置参考 | [[zh](docs/zh/configuration.md) / [en](docs/en/configuration.md)] |
| 运行时架构 | [[zh](docs/zh/architecture.md) / [en](docs/en/architecture.md)] |
| 实现状态 | [[zh](docs/zh/status.md) / [en](docs/en/status.md)] |
| Android root 模块 | [[zh](docs/zh/android-module.md) / [en](docs/en/android-module.md)] |
| 配置迁移 | [[zh](docs/zh/migration-config-pointer.md) / [en](docs/en/migration-config-pointer.md)] |
| 故障排查 | [[zh](docs/zh/troubleshooting.md) / [en](docs/en/troubleshooting.md)] |
| 开发与验证 | [[zh](docs/zh/development.md) / [en](docs/en/development.md)] |
| 共享模式服务部署与特权验收 | [[zh](docs/zh/servicectl.md) / [en](docs/en/servicectl.md)] |
| 共享模式兼容说明 | [[zh](docs/zh/shared-isolation-release.md) / [en](docs/en/shared-isolation-release.md)] |

## 致谢

特别感谢以下项目及其贡献者：

- [Droidspaces](https://github.com/ravindu644/Droidspaces-OSS)
- [lxc](https://github.com/lxc/lxc)
- [ruri](https://github.com/RuriOSS/ruri) — 参考其 `binfmt_misc` 注册机制及 ELF 架构魔数、掩码数据

## 许可证

KurumiContainerd 使用 GNU 通用公共许可证第 3 版（仅限此版本，`GPL-3.0-only`），详见
[LICENSE](LICENSE)。
