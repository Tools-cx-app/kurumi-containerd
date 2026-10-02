# KurumiContainerd

KurumiContainerd 是一个使用 Rust 编写的特权 Linux 容器运行时和命令行工具。它通过
Linux namespace、挂载、cgroup、网络及持久化 TOML 配置管理轻量级系统容器。

项目名中的 **Kurumi** 是日语“胡桃”（くるみ）的罗马字，后缀
**Containerd** 表明它是一个容器运行时项目。

本仓库只包含 CLI 和运行时，不包含 Android App。运行时在 Android 目标上编译
和执行时，仍可按配置暴露部分 Android 主机资源。

> KurumiContainerd 是容器运行时，不是安全沙箱。请只运行可信的 rootfs 和命令。

## 主要能力

- 目录、ext4 镜像、btrfs 镜像和块设备 rootfs
- 从本地 tar/ZIP 归档安装目录 rootfs 或稀疏 ext4 镜像
- Mount、PID、UTS、IPC 以及可选的 Network namespace
- Host、隔离、NAT 和已有网桥网络模式
- cgroup v1/v2 内存、CPU 和进程数限制
- Bind mount 和临时 OverlayFS
- 后台 monitor 与前台 PTY 控制台
- 无需外部 `nsenter` 的容器进入和命令执行
- 防 PID 复用的持久化运行状态
- seccomp 过滤和只读内核视图
- 可选的 user namespace 支持，用于容器内运行 Docker、Podman、Flatpak 和 Bubblewrap
- 不依赖 Android App 的可选 Android 主机集成

## 环境要求

- 支持所需 namespace 和挂载功能的 Linux 或 Android 内核
- root 权限
- Rust 1.85 或更高版本
- NAT/网桥网络所需的 `ip` 和 `iptables`
- 配置资源限制时可用的 cgroup v1 或 v2
- 包含 init 可执行文件的 Linux rootfs

启动容器前可检查主机能力：

```bash
sudo cargo run --release -p kurumi-containerd -- check
```

## 构建

```bash
cargo build --release
```

生成的二进制位于 `target/release/kurumi-containerd`。

## 自动发布

推送与 `Cargo.toml` 中 `[workspace.package].version` 一致的版本标签
（例如 `git tag v0.2.1 && git push origin v0.2.1`）后，自动构建并创建 GitHub Release。
带预发布后缀的版本标记为 prerelease，全部 10 个目标构建成功后才发布。

- GNU Linux（x86_64、aarch64、armv7、riscv64）：`.deb`、`.rpm`、`.tar.xz`。
- musl Linux（x86_64、aarch64）和 Android（CI 中的四种 ABI）：`.tar.xz`。
- `SHA256SUMS` 包含所有发布包的校验和。

压缩包包含可执行文件、许可证、README 和示例配置。
Debian/RPM 包将程序安装到 `/usr/bin`，文档和示例配置安装到
`/usr/share/doc/kurumi-containerd`。GNU 包要求系统 libc 不低于构建工具链使用的版本；
需要便携 Linux 程序时可使用 musl 压缩包。

## 快速开始

```bash
cp kurumi-containerd.example.toml kurumi-containerd.toml
$EDITOR kurumi-containerd.toml
sudo -H sh -c 'mkdir -p "$HOME/.kurumi-containerd" && exec vi "$HOME/.kurumi-containerd/config.json"'
```

将下面的 `file` 替换为 TOML 的实际绝对路径并保存：

```json
[
  {
    "name": "debian",
    "file": "/absolute/path/to/kurumi-containerd.toml"
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

固定读取实际运行进程的 `$HOME/.kurumi-containerd/config.json`。上述命令统一使用
`sudo -H`，让创建和运行都使用 root 的 HOME。JSON 是非空列表，`name` 必须唯一；
单项自动选中，多项使用 `kurumi-containerd --name debian start`。`name` 用于展示和选择，
运行时名称仍使用 TOML 的 `container.name`。`file` 的相对路径以 JSON 所在目录
为基准；TOML 内部相对主机路径仍以 TOML 所在目录为基准。

## 破坏性变更与迁移

已删除 `-c/--config`、`KURUMI_CONTAINERD_CONFIG` 及当前目录默认 TOML 查找入口。
保留原 TOML，在上述 JSON 中填写其绝对路径，并移除命令和脚本中的配置参数。
已有 JSON 请先备份再编辑。详细操作、sudo HOME 规则、验证和回退步骤见
[配置入口迁移指南](docs/migration-config-pointer.md)。

## 命令

```text
install ARCHIVE [--size SIZE] [--force]
                        从本地归档安装 rootfs
start [--foreground]   启动配置中的容器
stop                   优雅停止容器
restart [--foreground] 重启容器
enter [USER]           进入交互式登录，默认用户为 root
run COMMAND...          执行非交互命令
info                    显示运行状态和资源使用情况
pid                     输出容器 init PID
show                    列出运行时目录中的容器
scan                    验证运行进程并恢复缺失状态
check                   检查主机能力
```

## 文档

- [CLI 使用](docs/usage.md)
- [配置参考](docs/configuration.md)
- [运行时架构](docs/architecture.md)
- [实现状态](docs/status.md)

## 许可证

KurumiContainerd 使用 GNU General Public License v3.0 or later，详见
[LICENSE](LICENSE)。
