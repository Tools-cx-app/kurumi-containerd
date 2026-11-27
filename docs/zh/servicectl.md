# 共享模式中的 Dinit、s6-rc 与 servicectl

[简体中文](../zh/servicectl.md) | [English](../en/servicectl.md)

[文档目录](README.md)

[examples/servicectl](../../examples/servicectl) 中的资源包含前台 supervisor、Dinit
生命周期锚点、已初始化的 s6-rc 控制平面、受限 cgroup helper 和示例 unit。它们适用于
可信的 rootfs 工作负载。`shared-pid-ipc` 模式不支持普通 systemd 系统实例。

## 上游约定与前置条件

使用 `/s6/rc`、`/run/s6/state` 和 `/run/s6/compiled.servicectl`；仅运行其安装器并不会
启动实际的监督树。上游原生 Dinit 客户端要求 control protocol **v5 或更新版本**。请使用
带有原生 cgroup 支持的 Dinit 构建。

准备好的 rootfs 必须包含：

- `/usr/bin/python3`（版本 3.8 或更新）、`/bin/sh`、`/usr/bin/env`，以及 unit 所需的常见
  用户和组（包括 root）。
- 支持 protocol >=5、原生 `run-in-cgroup` 和 `dinitctl` 的 Dinit。
- s6、s6-rc 和 execline。上游需要 `/bin/s6-rc`、`/bin/s6-rc-compile`、
  `/bin/s6-rc-update` 和 `/usr/bin/execlineb`；采用 merged-usr 布局的发行版可以使用相应
  符号链接。`s6-svscan`、`s6-svscanctl` 和 `s6-rc-init` 也必须位于 PATH 中。
- 固定版本源码中的 servicectl、`sys-notifyd`、`sys-cgroupd`、`sys-dbusd`、`sys-logd`、
  `sys-propertyd`、`sysvisiond` 和 `sys-orchestrd`。提供的安装器需要构建主机上的 Go >=1.22，
  并以 rootfs 的架构为目标编译。Go 不需要加入 Kurumi，也不需要放进部署后的 rootfs。

宿主需要 root、mount/UTS/cgroup namespace、procfs、seccomp、cgroup namespace，以及独占且
可写的 cgroup v2 子树。不强制要求 `cgroup.kill` 或启用所有资源 controller；仅请求资源
限制时才需要相应的 controller。Kurumi 会创建一个空的 `kurumi-init` cgroup namespace
根目录，并在其 `bootstrap` 子组中运行 supervisor。安装资源时 rootfs 必须已停止。不要将
此安装器用于宿主根目录，也不要把宿主 `/run`、D-Bus、Dinit、servicectl socket 或宿主
cgroup 根目录 bind mount 进该 rootfs。可选的 D-Bus 激活需要单独配置私有 bus；示例不依赖它。
缺少 journald/syslog 可能导致日志输出回退或状态降级；示例不会 bind mount 宿主日志 socket。

## 构建运行时

共享模式要求 Kurumi 使用可选 Cargo feature `servicectl` 构建。CI 和标签发布产物会在
release profile 中启用它；debug 产物不启用。本地构建命令如下：

```sh
cargo build --release -p kurumi-containerd --features servicectl
```

该 feature 默认关闭。Cargo 不会因指定 `--release` 而自动启用它。未启用时，配置解析会
拒绝 `shared-pid-ipc`，共享运行时 API 也不会编入程序。

## 安装

准备架构匹配的 Linux rootfs，并先安装 Python/Dinit/s6/s6-rc/execline 软件包。软件包是否
可用取决于发行版；安装器不会调用发行版的软件包管理器。在 Kurumi 仓库的构建主机上执行：

```sh
git clone https://github.com/NEURAX-FX/servicectl.git /tmp/servicectl-src
git -C /tmp/servicectl-src checkout --detach 9bc7e99a4406a62b5dd745dac2f45472f6ed0b9a
sudo -E sh examples/servicectl/install.sh /srv/kurumi/debian/rootfs /tmp/servicectl-src
```

交叉编译时传入对应的 Go target，例如：
`sudo -E env GOOS=linux GOARCH=arm64 sh examples/servicectl/install.sh ROOTFS SOURCE`。
源码树必须干净并处于指定版本。安装器只向指定 rootfs 复制文件；它不会执行 rootfs 的 init，
也不会挂载宿主资源。

将以下条目合并到 `sudo -H` 所用的 HOME 配置列表：

```json
[
  {
    "runtime": {"stop_timeout_seconds": 90},
    "container": {
      "name": "debian-services",
      "rootfs": "/srv/kurumi/debian/rootfs",
      "init": "/usr/local/sbin/kurumi-services",
      "isolation": "shared-pid-ipc",
      "network": "host"
    }
  }
]
```

90 秒的停止时限用于覆盖正在运行的启动命令和示例中有界的回滚步骤。若服务关闭钩子耗时更久，
请相应增加该值。Kurumi 启动成功只代表入口程序已执行，不代表应用已就绪；请等待 ready 标记。

## 冒烟命令

以下命令会操作真实 rootfs 服务栈，是待执行的操作说明，不代表已通过测试：

以下基于断言的 runner 会创建临时 HOME 配置，并在准备好的临时 rootfs 中运行相同服务栈：

```sh
sudo python3 examples/servicectl/smoke.py target/release/kurumi-containerd /srv/kurumi/debian/rootfs
```

它会验证服务重启/停止钩子、持久化启用状态、Kurumi 重启和正常关机。脚本可能修改传入
rootfs 中的启用状态及示例标记。

```sh
sudo -H kurumi-containerd --name debian-services start
sudo -H kurumi-containerd --name debian-services run python3 -c \
  'import pathlib,time; p=pathlib.Path("/run/kurumi-services/ready"); end=time.monotonic()+60
while not p.exists():
 if time.monotonic()>end: raise SystemExit("bootstrap not ready")
 time.sleep(.1)'
sudo -H kurumi-containerd --name debian-services run servicectl start kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run servicectl is-active kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run servicectl status kurumi-demo.service --json
sudo -H kurumi-containerd --name debian-services run servicectl restart kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run servicectl stop kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run servicectl enable kurumi-demo.service
sudo -H kurumi-containerd --name debian-services restart
# 再次等待 /run/kurumi-services/ready，然后查询已启用的 unit。
sudo -H kurumi-containerd --name debian-services run servicectl is-active kurumi-demo.service
sudo -H kurumi-containerd --name debian-services run cat /var/lib/kurumi-demo/stopped
sudo -H kurumi-containerd --name debian-services stop
```

即使 workload 正在运行，`status` 在汇总健康状态为 degraded/unknown 时也会返回 3。应检查
JSON 诊断，并使用 `is-active` 确认 workload 状态；不能将失败的 status 当作健康检查通过。
`stopped` 文件记录正常执行过关闭钩子的 PID；rootfs 停止后也可以检查该文件。

## 启动、关闭与失败处理

`kurumi-services` 会以前台进程运行，作为 Kurumi 的生命周期锚点。它使用
`--system --container` 启动 Dinit，指定私有 `/run/dinitctl` 和服务路径，并验证原生控制连接。
Dinit 内部 anchor 会在没有应用活动时保持 manager 运行。s6 中的 `dinit` dependency 是空
bundle，因为该入口已负责启动 Dinit；不要再由 s6 service graph 启动第二个 manager。

入口程序依次编译源码 graph、启动 `s6-svscan`、运行 `s6-rc-init`、启动核心 daemon，最后启动
包含持久化 enabled-unit bundle 的 `default`。只有这些转换都成功后，才会创建
`/run/kurumi-services/ready`。每次 Kurumi 代际都需要全新的私有 `/run`。日志位于
`/run/kurumi-services/dinit.log`、`dinit-events.log` 和 `control-plane.log`；所有控制
socket 和运行时状态都留在私有 `/run` 中。`/s6/rc` 下的启用状态/服务定义以及 rootfs 中的
property 状态会在停止后保留。示例标记位于 `/var/lib/kurumi-demo`。

收到 SIGTERM、SIGINT 或 SIGHUP 时会请求有序关闭：先停止 enabled orchestrator，避免服务被
重新拉起；再请求 Dinit 关闭并等待业务服务钩子；然后停止剩余 s6-rc 服务，最后终止并回收
`s6-svscan`。部分启动失败或任一 supervisor 意外退出时也会执行相同回滚。仅在收到关闭请求
且成功完成时，入口程序才返回 0；启动、manager 丢失或清理错误会返回 1，并将结果写入
`/run/kurumi-services/stopped`。之后 Kurumi 会依靠独占的 cgroup 所有权处理残留后代，包括
double-fork/new-session 进程。SIGKILL 无法执行关闭钩子。请使用 Kurumi `restart` 替换运行代际。

monitor 会在 worker 结束并回收所有后代后移除 cgroup。`stop` 会等待这项清理，不会再次删除
同一个 cgroup。如果 monitor 退出，仍存活的 worker 会先移除 cgroup，再仅清除 PID、启动时间和
宿主 boot ID 均匹配的状态。

cgroup helper wrapper 总是传入 `--no-auto-mount`，并将根目录限制在
`/sys/fs/cgroup/servicectl.slice` 之下。cgroup namespace 根是空的 init cgroup；
`kurumi-services` 及其服务运行在 `bootstrap` 子组中。因此服务创建的 cgroup 保持在委派的
根目录之下，无法访问同级目录或宿主层级。上游重新生成 graph 时也会继续调用该 wrapper。

只支持 servicectl parser 已实现的指令。`.service` 文件不提供完整 systemd 语义、
`PrivateIPC=` 隔离、systemd PID 1 或发行版级启动兼容性。服务和脚本不得触发宿主关机、
`kill(-1)`、全局 `killall` 或清理共享 IPC。SysV IPC 和 POSIX 消息队列仍是共享的，Kurumi
不会删除其中的对象。

<a id="privileged-runtime-acceptance"></a>

## 特权运行时验收

```sh
cargo build --release -p kurumi-containerd --features servicectl
sudo python3 tests/shared_isolation.py target/release/kurumi-containerd
```

脚本会自行构建静态 mini supervisor 和临时 rootfs，测试真实运行时而不是 mock。它会检查
namespace 共享/缺失接口、run 退出状态、交互式 enter、实际持久化模式、double-fork/setsid
后代、嵌套 cgroup、重启、停止、monitor 丢失、init 崩溃、启动失败、可信 scan、两个实例和
不受影响的宿主 sentinel。CI 会在普通主机运行此脚本。缺少 mount 权限或可写 cgroup v2 时会
明确报告 **SKIP**，不计为已验证覆盖；脚本还会单独测试缺少 v2 时在 init 启动前明确拒绝。

仅在明确标记为可丢弃的 VM 内，才可额外运行无效 magic 的 reboot syscall 探测来检查 seccomp
拒绝行为。该探测不会提交有效的重启请求：

```sh
sudo env KURUMI_DISPOSABLE_VM=1 python3 tests/shared_isolation.py \
  target/release/kurumi-containerd --vm-reboot-filter-probe
```

要验证缺少 PID/IPC namespace 的环境，仍需使用真实的 VM/设备内核，并提供
`CONFIG_PID_NS=n` 和 `CONFIG_IPC_NS=n` 的内核配置证据。普通内核上的共享分支无法证明该环境。
旧内核清理路径同样必须在没有 `cgroup.kill` 的内核上验证；普通主机会单独报告该覆盖情况。
Android 需要针对设备验证 mount/UTS、cgroup 委派、seccomp 和 SELinux。未实际执行的测试不能
算作通过。
