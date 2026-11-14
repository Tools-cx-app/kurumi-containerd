# 运行时架构

[[zh](architecture_CN.md) / [en](architecture.md)]

[文档目录](README_CN.md)

KurumiContainerd 用七个工作区 crate 分别承担命令呈现、容器策略和宿主资源管理。
本文说明各层边界、资源归属，以及持久化状态可以被信任和使用的条件；这些机制并不构成
面向恶意工作负载的安全边界。

## 工作区边界

```text
crates/
  kurumi-containerd-cli/      command parsing and output
  kurumi-containerd-config/   strict JSON schema, path resolution, and validation
  kurumi-containerd-error/    shared error types, result aliases, and context
  kurumi-containerd-helper/   target-aware Linux and Android syscall wrappers
  kurumi-containerd-host/     host resources and platform integration
  kurumi-containerd-runtime/  lifecycle, isolation, and state
  kurumi-containerd-tui/      ratatui manager and CLI action dispatch
```

CLI 的包名和可执行文件名均为 `kurumi-containerd`，其他 crate 则划分内部实现职责。
依赖指向较低层服务：CLI 使用 TUI、runtime、configuration 和 host；TUI 使用 runtime
和 configuration；runtime 使用 host、configuration、helper 和 error；host 使用
configuration、helper 和 error；configuration 使用 error。helper 和 error 不依赖其他
工作区 crate。host 不导入 runtime 的生命周期状态或 init 家族策略；网络状态路径和关机
回调由 runtime 传入。

CLI 默认关闭 `tui` feature。执行 `cargo build -p kurumi-containerd` 时不会引入可选
TUI 依赖及其终端依赖；添加 `--features tui` 才会包含它们及 `tui` 子命令。
TUI 仍然是工作区成员，因此面向整个工作区的构建和测试仍会包含它。

error crate 定义 `ConfigError`、`RuntimeError`、结果类型别名、上下文和错误宏。
configuration 与 runtime 继续重新导出原有错误 API。host 和 runtime 共用
`RuntimeError`，策略回调跨越此边界时不需要转换错误；helper 保留底层 I/O 错误。
error 默认的 `runtime` feature 启用运行时错误、上下文、宏，以及可选的
procfs/JSON/ZIP 错误依赖。工作区关闭该默认 feature；configuration 沿用这一依赖设置，
host 和 runtime 则显式启用 `runtime`。因此，单独构建 configuration 不会通过 error
引入 procfs 或 ZIP。其他关闭默认 feature 的使用方也必须显式启用 `runtime`，才能使用
运行时错误。

公共接口遵循相同分工：configuration 通过 `Validate` 提供会修改配置的验证流程；runtime
通过 `ContainerRuntime` 提供面向用户的操作，通过 `ContainerQuery` 提供只读状态访问；
host 提供 `HostCheck` 和带类型的能力报告。命名空间、挂载、cgroup、进程和状态的内部实现
仍由具体类型承担。CLI 负责格式化宿主能力报告。CLI 和 TUI 也各自在呈现层格式化结构化的
`ContainerInfo` 字段；runtime 的 `Display` 和 `display_lines` 作为兼容 API 保留，
新增格式化行为不应放到这里。

## 配置模块

configuration crate 从 `lib.rs` 导出模式类型。加载与验证保持在一起，环境解析和注册表
选择则分别放在独立模块中：

```text
src/
  lib.rs          public configuration API
  model.rs        serializable configuration schema and defaults
  config.rs       loading, path resolution, and validation
  environment.rs  environment assignment parsing
  registry.rs     configuration registry loading and selection
```

调用方使用 `kurumi_containerd_config`，不依赖这一私有布局。注册表实现在
`registry.rs` 中，旧文档中的 `pointer.rs` 已不是当前文件名。

## 生命周期

启动时先验证配置并取得该容器的生命周期锁，再准备 rootfs、分配长期资源并创建监控进程。
每一代启动都通过一个中间进程创建新的 PID 命名空间和可选网络命名空间，然后 fork 出 init。

监控进程在容器内部重启期间持续持有 rootfs 挂载、控制台 PTY、cgroup，以及适用的 Android
SELinux 状态。init 和监控进程的身份记录包含宿主启动 ID、进程启动时间及 PID 命名空间
inode；仅有数值 PID 不能证明预期进程仍然存活。向 init 发送信号和等待退出时，会在可用时
使用 pidfd。

停止与重启共用每容器生命周期锁。停止操作在等待监控进程时，还会持有独立的停止请求锁。
内部重启会等待普通生命周期锁持有者，仅在停止请求处于活动状态时取消。关闭停止请求的
描述符就会清除请求，即使调用方崩溃也一样。

进入运行中容器的命令会捕获 init 实际使用的网络与 cgroup 命名空间及 cgroup 成员关系，
不会根据当前 JSON 重新推导。它们通过已验证监控进程的挂载视图打开 cgroup 目录，因此
仍能访问在私有命名空间中引导建立的层级；命令加入所捕获容器分组下的执行叶节点，不改变
资源限制。修改网络或资源配置只影响下一次启动，不影响进入当前启动代次的命令。

## 运行时模块

运行时代码按职责分组：

```text
src/
  container/  behavior and policy applied inside the container
  runtime/    lifecycle orchestration, execution, state, and supervision
```

下表路径相对于 runtime crate 的 `src/`：

| 模块 | 职责 |
| --- | --- |
| `runtime/lifecycle.rs` | 启动、监控、启动代次、关机和清理 |
| `runtime/boot.rs` | 命名空间内部挂载、根目录切换、嵌套沙箱挂载、环境和 init 执行 |
| `runtime/supervisor.rs` | 等待与重试、信号策略和子进程监督辅助逻辑 |
| `runtime/query.rs` | 为详情、PID 和存活容器列表提供只读 `ContainerQuery` 接口 |
| `runtime/state/mod.rs` | 公共状态类型、进程身份和恢复策略 |
| `runtime/state/store.rs` | 私有 `StateStore`：可信持久化、状态路径和锁 |
| `runtime/state/info.rs` | 状态和资源用量 |
| `runtime/execute.rs` | 命名空间进入、命令执行和交互登录 |
| `runtime/install.rs` | rootfs 安装的生命周期协调 |
| `runtime/backup.rs` | root 权限检查、生命周期加锁和仅限停止状态的 rootfs 导出 |
| `container/environment.rs` | 确定性的 init 与会话环境 |
| `container/init.rs` | init 家族检测和关机协议 |
| `container/security.rs` | seccomp 和受保护的内核视图 |

当前源码中的查询与持久化模块是文件（`query.rs` 和 `state/store.rs`），不是
`query/` 或 `state/store/` 模块目录。

宿主资源位于 host crate 的 `src/`：

| 模块 | 职责 |
| --- | --- |
| `process.rs` | pidfd 句柄和 procfs 进程身份辅助逻辑 |
| `terminal.rs` | PTY 分配、描述符传递和控制台代理 |
| `rootfs/mod.rs`、`rootfs/install.rs` | rootfs 准备、备份入口和归档安装 |
| `archive.rs`、`archive/backup.rs` | 私有归档解包和流式导出 |
| `network/mod.rs` | veth、网桥、NAT、转发和回滚 |
| `network/state.rs` | 网络状态与 NAT 租约持久化 |
| `cgroup.rs` | cgroup v1/v2 限制和层级检测 |
| `android.rs` | Android 宿主集成，仅在 Android 目标上公开 |

宿主资源 API 只接收实际需要的配置：`Rootfs` 接收 rootfs 路径；网络设置接收容器名、网络
模式和网络选项。接收完整 Config 的构造方法作为 runtime 以外调用方的兼容适配器保留。
资源归属仍由父模块管理；网络持久化与网络设置、清理的协调逻辑分离。

终端代理通过 helper 的安全 `terminal::poll` 包装轮询描述符。helper 处理目标原生的
`pollfd`、事件标志和经过检查的 `nfds_t` 数量，并返回事件和等待中断；host 负责
100 ms 等待、中断重试、输入缓冲、关机回调和后台 PTY 排空策略。后台输出即使被丢弃也必须
持续读取，否则 PTY 缓冲区填满后可能阻塞 init 或其服务。

目录模块用于容纳相关生产子模块：host 的 `network`、`rootfs`，以及 runtime 的
`container`、`runtime` 和 `runtime/state`。内聚的叶模块使用 `<module>.rs`；终端、
配置和 TUI 渲染辅助逻辑在共享生命周期或渲染状态时保持在一起。测试文件通过必要的显式
路径保留其原有位置和可见性。

### TUI 后端与呈现

TUI 将渲染、输入状态和操作验证与私有后端分离。`registry.rs` 仅定义 `Entry` 模型。
`backend.rs` 负责加载与刷新配置，不持久化 UUID；它通过 `ContainerQuery` 获取运行时状态，
再通过图形终端或同一 CLI 可执行文件的输出捕获子进程分派操作。

`CommandTask` 同时持有子进程和输出文件。轮询返回退出状态，以及最多 1 MiB 的合并
stdout/stderr，使用有损 UTF-8 解码。临时文件与轮询 I/O 错误向上传播；启动子进程失败则
作为可恢复的界面反馈。应用负责单个可选任务、忙碌状态、刷新时机和反馈，不自行构造命令
或读取捕获文件。后端沿用 [CLI 用法](usage_CN.md)中说明的 CLI 参数和终端选择行为。

## 嵌套用户命名空间

启用 `container.security.allow_user_namespaces` 后，`runtime/boot.rs` 会在常规容器
文件系统设置之前挂载 `/run`，并在 `pivot_root` 后创建两个额外的文件系统实例：

| 挂载点 | 标志 |
| --- | --- |
| `/run/kurumi-containerd/proc` | 可写 procfs，带 `nosuid,nodev,noexec` |
| `/run/kurumi-containerd/sys` | 只读 sysfs，带 `ro,nosuid,nodev,noexec` |

常规 `/proc` 和 `/sys` 仍保留配置的加固和遮罩。这些额外实例用于满足内核可见性要求，
使 Docker、Podman 或其他嵌套沙箱能够从非特权用户命名空间挂载 procfs/sysfs。未遮罩的
proc 视图可能暴露宿主内核 sysctl。只对可信容器启用该选项；它并不构成恶意工作负载的
安全边界。

## 状态与信任

运行时文件和目录使用严格的所有权与权限。状态更新使用不跟随符号链接的打开方式、随机
临时文件、fsync 和原子重命名。恢复要求同时存在受保护的宿主恢复记录，以及容器内部
`/run/kurumi-containerd` 下匹配的身份标记；仅扫描到存活的 PID 1 并不足够。

状态发布、删除和恢复提交共用状态目录中的每容器持久化锁。恢复在持锁时再次比较原始记录，
丢弃已被生命周期操作修改或删除的候选项。此锁独立于生命周期锁，因此安装或备份期间的
扫描不会重复进入生命周期锁，也不会干扰重启协调。`StateStore` 统一管理这些路径、锁和
可信持久化操作；进程身份与恢复策略仍由外围状态模块负责。

运行时安装 classic-BPF seccomp 过滤器，也可以通过只读挂载和绑定遮罩保护内核视图。
容器 root 仍保留其 capabilities，特权容器机制仍共享宿主内核。这些控制措施不会让
KurumiContainerd 成为面向恶意工作负载的沙箱。请使用可信 rootfs 和命令，并在部署前阅读
[当前限制](status_CN.md#当前限制)。
