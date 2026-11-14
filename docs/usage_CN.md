# CLI 使用

[[zh](usage_CN.md) / [en](usage.md)]

[文档目录](README_CN.md)

本文介绍如何选择已配置的容器、安装 rootfs、管理生命周期，以及导出和恢复容器。
KurumiContainerd 需要执行特权主机操作；请只运行可信的 rootfs 和命令。

<a id="invocation"></a>
## 调用与容器选择

```text
kurumi-containerd [--name NAME] [--verbose] COMMAND
```

登记文件为 `$HOME/.kurumi-containerd/config.json`，内容是非空 JSON 列表，各项包含完整配置，
且 `container.name` 必须唯一。大多数命令在只有一项时自动选择该项。存在多项时，
在命令之前传入 `--name NAME`；名称必须完全匹配，区分大小写。
`backup CONTAINER_ID OUTPUT` 是特例：位置参数 ID 直接选择 `container.name`，
同时使用 `--name` 会被拒绝。只有 `check` 不加载配置。

本文示例使用 `sudo -H`，因此请在 root 的 HOME 下创建登记文件。更改 HOME 就会选择
另一份登记文件。大多数操作需要 root 来创建 namespace、挂载、设备、cgroup 或网络接口。
JSON 和路径规则见[配置参考](configuration_CN.md)，已删除的配置入口见
[迁移指南](migration-config-pointer_CN.md)。

```text
install ARCHIVE [--size SIZE] [--force]
backup CONTAINER_ID OUTPUT
start [--foreground]
stop
restart [--foreground]
enter [USER]
run COMMAND...
info
pid
show
scan
check
tui
```

只有启用下文所述可选 feature 的构建才提供 `tui`。

<a id="output-and-diagnostics"></a>
## 输出与诊断

命令结果和成功提示以纯文本输出到 stdout，不带时间戳或日志级别。`pid` 只输出 init PID 和换行：

```bash
pid=$(sudo -H kurumi-containerd pid)
```

debug 构建将诊断日志写入 stdout，release 构建写入 stderr。默认级别是 INFO；
`--verbose` 或 `-v` 还会启用 DEBUG。只有日志流连接终端且未设置 `NO_COLOR` 时才使用颜色。
失败会包含完整错误原因链并返回非零状态；`run` 和 `enter` 保留所执行命令的退出状态。
关闭读取命令结果的管道不会引起 panic。

前台 start/restart 在容器退出后报告完成；后台 start/restart 在启动成功后报告 init PID。
脱离终端的 monitor 在启动后将标准流重定向到 `/dev/null`，因此重定向 stderr 不能提供
持久的后台容器日志。TUI 会捕获非交互操作的结果和诊断输出。

<a id="terminal-manager"></a>
## 终端管理界面

`tui` Cargo feature 默认关闭。构建时启用它以包含管理界面及其依赖：

```bash
cargo build --release -p kurumi-containerd --features tui
sudo -H kurumi-containerd tui
```

未启用该 feature 时不存在 `tui` 子命令。ratatui 管理界面列出 HOME 登记文件中的所有项，
包括已停止的容器。每个操作都调用同一个 CLI 可执行文件并使用同一个 HOME 登记文件。
可用操作包括 install、backup、start/stop/restart（含前台模式）、enter、run、
info、pid、show、scan 和 check。登记列表为空时仍可使用 Check host。

| 场景 | 按键与行为 |
| --- | --- |
| 容器列表 | `j`/`k` 或上下方向键选择容器。 |
| 操作选择 | `h`/`l` 或左右方向键选择操作；Enter 打开对话框。不可用操作会说明原因。 |
| 面板 | `Tab`/`Shift+Tab` 在容器、详情、输出之间切换焦点。PageUp/PageDown 分别滚动当前聚焦的详情或输出。切换容器会重置详情滚动位置。 |
| 对话框字段 | `Tab`/`Shift+Tab` 切换字段；在 `run` 的最后一个参数处按 Tab 会添加参数。选中 install 的 force 字段时按空格切换。 |
| 提交 | Enter 提交。stop、restart、scan 和强制 install 需要再次按 Enter 确认，确认提示会标明目标。校验错误显示在对话框内。 |
| 导航 | Escape 取消对话框；`r` 重新加载登记文件；没有命令运行时 `q` 退出。 |

状态会定期刷新。较窄终端将列表和详情上下排列，过小终端显示调整尺寸提示。
备份时选择已停止的容器，选择 Backup，再输入一个尚不存在、以 `.zst`、`.xz`、`.zip`
或 `.tar` 结尾的路径。所选 `container.name` 用作 ID。`run` 使用独立的可执行文件和
参数字段，不解析 shell 引号或管道。捕获输出最多保留前 1 MiB。

在 Linux 上，交互和前台命令会打开独立图形终端，依次检测 `xterm`、`kitty`、`alacritty`。
可将 `KURUMI_CONTAINERD_TERMINAL` 设置为其中一个可执行文件名或其路径，覆盖自动检测。
终端会保持打开以便阅读结果，完成后手动关闭。在无图形环境的 Linux 和 Android 上，
这些操作不可用，其他非交互操作仍可使用。若某项配置为 `container.foreground: true`，
普通 start/restart 操作也会使用独立终端。

<a id="install-a-local-rootfs"></a>
## 安装本地 rootfs

`install` 只读取本地归档，不下载 rootfs，也不接受 URL 或 registry 来源。程序通过文件头
而非扩展名识别 tar、gzip 压缩 tar、XZ 压缩 tar、Zstandard 压缩 tar 或 ZIP。
所选配置项使用目录目标时：

```bash
sudo -H kurumi-containerd install ./debian-rootfs.tar.zst
```

创建稀疏 ext4 镜像时，配置 `container.rootfs_image` 并指定逻辑容量。
支持 `M`、`G`、`MiB`、`GiB` 等二进制容量后缀：

```bash
sudo -H kurumi-containerd install ./debian-rootfs.tar.zst --size 8G
```

容器必须已停止。现有目标需要 `--force`；替换内容会先暂存，只有提取及 init 校验成功后
才重命名提交。镜像安装还需要 `mke2fs` 或 `mkfs.ext4`、loop 设备和挂载权限。
镜像目标必须指定 `--size`，目录目标则禁止使用它。

归档条目直接安装到 rootfs 顶层：`sbin/init` 外不能再包一层发行版目录。
ZIP 无法可靠保留 UID/GID 或设备节点。系统 rootfs 归档推荐使用 tar；
选择备份格式前，还应阅读下一节的恢复元数据限制。

<a id="back-up-a-container"></a>
## 备份容器

导出前先停止源容器：

```bash
sudo -H kurumi-containerd --name debian stop
sudo -H kurumi-containerd backup debian ./debian.zst
```

`CONTAINER_ID` 是 JSON 中精确匹配、区分大小写的 `container.name`，不是 UUID。
即使登记列表包含多项，也直接以此选择目标，无需 `--name`；同时指定 `--name` 会报错。
备份不会改变名称或 UUID。输出后缀决定归档格式：

| 后缀 | 内容 |
| --- | --- |
| `.zst`, `.tar.zst` | Zstandard 压缩 tar |
| `.xz`, `.tar.xz` | XZ 压缩 tar |
| `.zip` | ZIP |
| `.tar` | 未压缩 tar |

备份需要 root 且容器必须停止。导出内容为配置的 rootfs 目录或镜像内部的文件系统，
不是原始镜像。它不包含 config.json、运行状态、bind mount 源目录或已丢弃的临时 OverlayFS
修改。镜像源需要与启动相同的 loop 设备和挂载支持。生命周期锁会阻止此容器在导出期间启动；
同时也应避免外部程序修改源文件系统。

提前在 rootfs 外创建输出目录。任何现有输出路径（包括符号链接）都不会被覆盖。
导出流先写入权限为 `0600` 的私有临时文件，成功后才发布；普通失败会清除不完整文件。
请先卸载 rootfs 内的子挂载。遇到内核文件系统或特殊文件（设备、socket、FIFO）时会拒绝备份，
而不是静默跳过。

Tar 及压缩 tar 保留所有权、权限、时间戳、硬链接、符号链接和 xattr。
ZIP 保留普通权限及安全的相对符号链接，但丢失所有权、时间戳、xattr、特殊权限位和硬链接关系；
命令会对此发出警告。ZIP 遇到绝对或逃逸符号链接、非 UTF-8 或有歧义的路径时会拒绝处理，
不会修改它们。系统 rootfs 备份推荐使用 `.zst`、`.xz` 或 `.tar`。

恢复时选择目标配置项，再使用 `install`：

```bash
sudo -H kurumi-containerd --name restored install ./debian.zst
```

镜像目标仍需 `--size`，现有目标仍需 `--force`。当前安装器只恢复普通文件的 tar xattr，
其 PAX 读取器无法恢复包含换行的属性值。若需恢复目录或符号链接的 xattr，或含换行的属性值，
请使用启用 xattr 恢复的外部 tar 实现，例如 GNU tar 的 `--xattrs --xattrs-include='*'`。
请单独保存 config.json：备份始终不会将其打包。

<a id="lifecycle"></a>
## 生命周期

```bash
sudo -H kurumi-containerd start
sudo -H kurumi-containerd start --foreground
sudo -H kurumi-containerd restart
sudo -H kurumi-containerd stop
```

后台 start 会脱离一个 monitor 进程。前台 start 将容器控制台连接到当前终端；
依次按 Escape、Ctrl-Q 可请求关机。`restart` 先停止再启动容器，也接受 `--foreground`。

stop 根据检测到的 init 家族选择关机协议，并在 `runtime.stop_timeout_seconds` 后升级处理。
自定义 init 先收到 `SIGTERM` 以执行清理，超时仍未退出则收到 `SIGKILL`。
Alpine 使用 BusyBox 作为 PID 1 时，即使服务由 OpenRC 管理，也会收到 BusyBox 关机信号。

`.github/workflows/tests.yml` 中的特权生命周期 CI 在 Ubuntu 上运行，使用
`images.linuxcontainers.org` 提供的 Debian Bookworm（systemd）和 Alpine 3.22（OpenRC）
rootfs 归档。下载内容按镜像索引中的 SHA-256 校验。测试用 CLI 安装归档，覆盖 init 就绪、
命令执行及退出状态、重启、优雅停止、进程/状态/cgroup 清理、挂载隔离，以及停止后重新启动。
由 init 管理的服务记录启动和关机，因此强制终止不算优雅停止成功。网络模式为 `none`。

使用与主机架构匹配的 LXC rootfs 运行同一集成脚本：

```bash
cargo build --release --locked -p kurumi-containerd
sudo target/release/kurumi-containerd check
sudo python3 tests/container_lifecycle.py target/release/kurumi-containerd ./rootfs.tar.xz systemd
# Alpine 请将 systemd 改为 openrc。
```

脚本需要 root、namespace/挂载权限、Python 3 和归档文件。它会创建隔离的 HOME 登记文件
及临时 rootfs，在退出时停止容器并清除临时文件。无需安装 LXC 或 Incus。

<a id="nested-sandboxing"></a>
## 嵌套沙箱

需要创建非特权 user namespace 的工作负载可启用以下选项：

```json
{
  "container": {
    "security": {
      "allow_user_namespaces": true
    }
  }
}
```

适用示例包括 Docker userns-remap、Podman、Flatpak 和 Bubblewrap。运行时保留常规
`/proc` 和 `/sys` 挂载，并额外挂载新的 `/run/kurumi-containerd/proc`（可写）和
`/run/kurumi-containerd/sys`（只读）视图。额外的 proc 视图可能暴露主机内核 sysctl，
降低隔离程度。仅应为可信容器启用此选项。

<a id="enter-and-run"></a>
## 进入容器与执行命令

```bash
sudo -H kurumi-containerd enter
sudo -H kurumi-containerd enter developer
sudo -H kurumi-containerd run uname -a
sudo -H kurumi-containerd run sh -c 'id && mount'
```

`enter` 以指定用户打开交互式登录，默认用户为 `root`。`run` 直接执行其余参数。
使用管道、重定向或复合命令时，请显式调用 shell。

两者都加入运行中容器实际使用的 namespace 和 cgroup。修改 config.json 中的网络或
资源设置只在下次启动时生效，不会改变进入当前实例的命令所使用的隔离或资源限制。

<a id="inspect-and-recover"></a>
## 查看与恢复状态

```bash
sudo -H kurumi-containerd info
sudo -H kurumi-containerd pid
sudo -H kurumi-containerd show
sudo -H kurumi-containerd scan
```

`info` 显示容器身份、init 与 monitor PID、init 系统、运行时长、内存用量、进程数、
代次、rootfs 和 UUID。`pid` 返回 init PID。`show` 读取配置工作目录中的实时状态。
恢复缺失状态时，`scan` 在 procfs 中搜索 namespace PID 1 进程；只有容器内身份与
root 所有的恢复记录一致时才会恢复。

<a id="host-checks"></a>
## 主机能力检查

```bash
sudo kurumi-containerd check
```

`check` 不需要登记文件。它探测 mount、PID、UTS、IPC、network namespace，
然后报告 OverlayFS、cgroup v2、pidfd、`ip`、`iptables` 的可用性。
必需的 namespace 探测失败时返回错误。
