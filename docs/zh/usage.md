# CLI 使用

[简体中文](../zh/usage.md) | [English](../en/usage.md)

[文档目录](README.md)

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

本文示例使用 `sudo -H`，因此使用 root 的 HOME 下的登记文件。`install` 可自动创建
登记文件及缺失项。更改 HOME 就会选择另一份登记文件。大多数操作需要 root 来创建
namespace、挂载、设备、cgroup 或网络接口。
JSON 和路径规则见[配置参考](configuration.md)，已删除的配置入口见
[迁移指南](migration-config-pointer.md)。

```text
install ARCHIVE [--size SIZE] [--force]
backup CONTAINER_ID OUTPUT
start [--foreground]
stop
restart [--foreground]
enter [USER] [--shell COMMAND]
run COMMAND...          # 别名：exec
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
cargo build --release -p kurumi-containerd --features tui,servicectl
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
| 容器内服务 | 选中运行中的容器，按 `s` 打开服务列表。上下键选择服务，Enter 查看状态，`r` 刷新，Esc 返回。 |

服务页面通过所选容器的 `run` 通道探测 systemd 或 OpenRC，在当前终端显示运行状态、启动配置和命令输出。
按 `s` 启动、`x` 停止、`R` 重启、`e` 启用或 `d` 禁用选中的服务，再按 Enter 确认。操作完成后自动刷新列表。
OpenRC 的启用和禁用只修改 `default` runlevel。systemd 服务可用 `J` 查看最近 100 条 journal 日志。
容器内需要 `/bin/sh` 和相应的管理工具。此页面仅需 `tui` feature，可在无图形桌面的终端中使用。

状态会定期刷新。较窄终端将列表和详情上下排列，过小终端显示调整尺寸提示。
备份时选择已停止的容器，选择 Backup，再输入一个尚不存在、以 `.zst`、`.xz`、`.zip`
或 `.tar` 结尾的路径。所选 `container.name` 用作 ID。`run` 使用独立的可执行文件和
参数字段，不解析 shell 引号或管道。捕获输出最多保留前 1 MiB。
Enter 对话框提供可选的 Shell command 字段：填写 `/bin/ash` 或 `/bin/busybox ash`
可指定 Alpine 登录 shell，留空则使用默认登录方式。

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

登记文件不存在、列表为空，或 `--name NAME` 指定的项不存在时，`install` 会先保存
默认配置。新项以 NAME 命名；未指定名称且登记为空时使用 `default`。
默认目录为 `$HOME/.kurumi-containerd/NAME.rootfs`；传入 `--size` 时改用
`NAME.img` 镜像。已有项保留原配置，多项时仍必须指定 `--name`，非法 JSON 不会被覆盖。
配置先于解包保存，因此安装失败后仍可修改配置或重试。

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

生命周期回归覆盖 systemd 和 OpenRC 的就绪、执行、重启、优雅停止及清理。
运行脚本、环境要求与覆盖范围见[开发与验证](development.md)。

缺少 PID/IPC namespace 的内核可显式选择 `container.isolation: "shared-pid-ipc"`，
并使用前台 supervisor。该模式需要 `servicectl` feature，不支持普通 systemd 系统实例，
始终要求专属可写的 cgroup v2，但不要求 `cgroup.kill`。它不会替代失败的独立隔离启动。
详见[隔离配置](configuration.md#pid-and-ipc-isolation)和
[Dinit/s6/servicectl 部署](servicectl.md)。共享模式的特权验收命令与环境覆盖限制见
[开发与验证](development.md#特权集成验证)。

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
sudo -H kurumi-containerd --name alpine enter --shell /bin/ash
sudo -H kurumi-containerd --name alpine enter --shell '/bin/busybox ash'
sudo -H kurumi-containerd run uname -a
sudo -H kurumi-containerd run sh -c 'id && mount'
```

`enter` 以指定用户打开交互式登录，默认用户为 `root`。`run` 直接执行其余参数。
使用管道、重定向或复合命令时，请显式调用 shell。

可使用 `enter --shell /bin/ash` 显式指定登录 shell，例如进入 Alpine。
命令须以容器内的可执行文件绝对路径开头，可带参数，例如 `--shell '/bin/busybox ash'`。
CLI 中需引用整个命令；TUI 的 Shell command 字段直接填写 `/bin/busybox ash`，无需外层引号。
支持用 shell 风格的引号和反斜杠对参数分组，不执行变量展开、命令替换或管道。
路径自身含空格时，需在命令内部引用该路径。指定后跳过 `su` 和 `/etc/passwd` 的 shell 字段，
仍使用所选用户的 UID、GID、附加组、主目录和登录环境。`SHELL` 设为可执行文件路径，
在指定参数后追加 `-l`，因此 BusyBox 实际收到 `busybox ash -l`。
不指定 `--shell` 时，先尝试容器内的 `su -l`，再尝试 `/etc/passwd` 中的 shell。
该选项仅作用于当前会话，不修改 `/etc/passwd` 或 config.json。即使 shell 文件存在，
缺失脚本解释器或动态加载器仍可能导致 `No such file or directory`。
容器 init 和命令直接调用内核 `execve`，避免宿主预加载库（如 `termux-exec`）改写容器路径。
Linux 上可运行 `python3 tests/exec_interposition.py`，通过预加载的 `execve` 拦截库验证该行为；
需要 Cargo 和宿主 C 编译器，无需 root。

两者都加入运行中容器实际使用的 namespace 和 cgroup，并以内存记录的实际隔离模式为准。
共享模式不会打开或加入缺失的 PID/IPC namespace 接口。修改 config.json 中的隔离、网络或
资源设置只在下次启动时生效，不会改变进入当前代次的命令所使用的隔离或资源限制。

<a id="inspect-and-recover"></a>
## 查看与恢复状态

```bash
sudo -H kurumi-containerd info
sudo -H kurumi-containerd pid
sudo -H kurumi-containerd show
sudo -H kurumi-containerd scan
```

`info` 显示容器身份、实际隔离模式、init 与 monitor PID、init 系统、运行时长、内存用量、
进程数、代次、rootfs 和 UUID。`pid` 返回 init PID。共享模式统计专属 cgroup 的递归成员，
而不是宿主总量。`show` 列出 daemon 内存中跟踪的容器，`scan` 校验这些记录的进程身份。
运行状态与恢复元数据均保存在内存。CLI 在 socket 连接失败时自动拉起 daemon 并打印日志。
Linux socket 为 `/var/kurumi/socket.sock`，Android 为 `/dev/kurumi/socket.sock`。
请求日志追加到 socket 同目录的 `daemon.log`。daemon 脱离终端并由 PID 1 收养。
daemon 退出会终止其容器并触发资源清理，再次启动使用空注册表。配置 JSON 和 rootfs 内容
继续保存在磁盘。

<a id="host-checks"></a>
## 主机能力检查

```bash
sudo kurumi-containerd check
```

`check` 不读取 HOME 或配置，报告 namespace、OverlayFS、cgroup v2、pidfd、
`ip` 和 `iptables` 能力，并区分独立/共享模式前提。共享模式还要求 cgroup namespace
和专属可写的 cgroup v2。缺少 PID/IPC namespace 不会单独阻止共享模式，非 host 网络
才需要 network namespace。`start` 仍会检查所选模式、rootfs/init、安全机制和 cgroup
委派；通用检查成功不代表每个配置都能启动。
