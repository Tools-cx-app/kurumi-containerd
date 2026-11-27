# 共享 PID/IPC 模式兼容性说明

[简体中文](../zh/shared-isolation-release.md) | [English](../en/shared-isolation-release.md)

[文档目录](README.md)

可选 Cargo feature `servicectl` 控制共享 PID/IPC 支持，默认关闭。CI 和标签发布只在
release 产物中启用该 feature；本地 release 构建必须显式传入
`--features servicectl`。未启用时，`IsolationMode::SharedPidIpc` 和共享运行时 API
不会编入程序，该构建也无法加载共享模式配置和状态记录。管理现有共享代际时，请使用
启用了该 feature 的二进制文件。

## 配置与公开结构

`container.isolation` 使用严格的 kebab-case JSON：默认值为 `isolated`，共享模式必须显式
设置为 `shared-pid-ipc`。已有配置仍使用隔离模式。未知值会导致验证失败；namespace
操作失败时不会自动降级。示例配置显式写出 `isolated`。

公开 Rust 结构 `ContainerConfig` 新增 `isolation: IsolationMode`。外部 struct 字面量需要
提供此字段，或使用默认值更新语法。`ContainerState` 新增持久化的实际模式和共享代际身份
元数据（worker PID/启动时间、mount namespace inode 和 cgroup 路径）。
`pid_namespace_inode` 改为 `Option<u64>`；已有 JSON 数字仍会反序列化为 `Some`，缺少模式
字段的旧记录则默认为隔离模式。外部 Rust 状态构造代码和使用 inode 字段的代码需要相应
更新。共享模式记录必须包含该模式要求的身份数据；namespace 接口不存在并不意味着可以
依据任意 PID 恢复状态。

CLI/TUI 状态输出会显示实际隔离模式。共享模式的进程数和内存统计来自专用的递归 cgroup
子树。该显示不承诺稳定的机器可读格式。即使当前 JSON 已更改，运行实例的进入、检查、
停止和恢复仍依据持久化的模式处理。

## 运行时与检查

共享模式始终要求独占且可写的 cgroup v2，即使没有设置资源限制；它没有 cgroup v1 后端。
只有请求相应资源限制时才需要启用对应 controller。`cgroup.kill` 是可选项；旧内核通过
可信的进程句柄递归清理成员。init 和 exec/login 会拒绝 reboot syscall。共享 supervisor
收到 SIGHUP 时会退出，不会触发隐式 namespace 重启。重启前必须清空并回收旧代进程。

`check` 不依赖 HOME 或配置文件，报告主机能力和各模式的前置条件，并依据基础运行时能力
决定退出状态。仅缺少 PID/IPC namespace 不会使共享模式不可用。非 host 网络模式还要求
network namespace。启动时仍会检查实际配置、init 兼容性、安全设施和 cgroup 委派情况。

共享模式仅适用于可信服务。它会暴露宿主可见的 PID，并共享 SysV IPC/POSIX 消息队列。
私有 `/dev/shm`、cgroup 和 seccomp 都不能提供 PID/IPC 隔离。普通 systemd 系统实例和
依赖 PID 1 特性的 init 不受支持。自定义 supervisor 必须以前台运行，并避免触发宿主关机、
全局信号和共享 IPC 清理。Kurumi 不会删除共享 IPC 对象。如果所有 supervisor 同时遭到
SIGKILL、宿主关机或内核故障，用户态程序无法保证自动清理。

## 部署指南与验收证据

[servicectl 指南](servicectl.md)固定了检查过的上游提交，并提供 rootfs 资源、运行依赖和
具体命令。Dinit/s6/servicectl 仍是 rootfs 中的外部程序；Kurumi 的 Rust 依赖不包含 Go。
这里只支持 servicectl 已实现的 unit 解析语义，不等同完整的 systemd 隔离能力。

主机集成工作流会运行 `tests/shared_isolation.py`。脚本只有在实际观察到断言成立时才报告
PASS；缺少 mount/cgroup 前置条件或未实际覆盖的环境会报告 SKIP。可选 VM 过滤器探测使用
无效的 reboot magic，不会发出实际重启请求。

本文不声称已执行部署，也不声称已验证禁用 PID/IPC namespace 的内核、旧内核清理路径、
目标 Android 设备或 VM 过滤器探测。执行验收时，应单独记录实际工具结果；检查源码和 CI
是否接入不能代替运行时通过证据。
