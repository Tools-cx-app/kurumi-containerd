# 文档目录

[[zh](README_CN.md) / [en](README.md)]

[项目概览](../README_CN.md)

KurumiContainerd 在 Linux 和 Android 上管理特权 Linux 系统容器。
先阅读项目快速开始，再按下表查阅操作、配置和实现细节。
每篇指南都有完整的中英文版本；页面顶部的语言链接切换当前主题的语言。

| 指南 | 内容 | 语言 |
| --- | --- | --- |
| CLI 使用 | 容器选择、TUI、安装、备份恢复、生命周期、命令执行和诊断 | [[zh](usage_CN.md) / [en](usage.md)] |
| 配置参考 | JSON 结构、路径、字段默认值、网络、资源、安全、环境变量、挂载和 Android 选项 | [[zh](configuration_CN.md) / [en](configuration.md)] |
| 运行时架构 | crate 边界、生命周期、主机资源、状态持久化和恢复信任 | [[zh](architecture_CN.md) / [en](architecture.md)] |
| 实现状态 | 已实现能力和当前限制 | [[zh](status_CN.md) / [en](status.md)] |
| Android root 模块 | Magisk、KernelSU、APatch 安装、持久化文件和开机启动 | [[zh](android-module_CN.md) / [en](android-module.md)] |
| 配置迁移 | 将旧 TOML 和 name/file 指针转换为单一 JSON 配置，验证与回滚 | [[zh](migration-config-pointer_CN.md) / [en](migration-config-pointer.md)] |

全新安装时，先配置容器条目、检查主机，再安装本地 rootfs 并启动容器。
旧安装如果仍使用原配置入口，应先停止容器并按迁移指南转换。
请在 rootfs 归档之外单独备份 `config.json`。

运行时共享主机内核，容器 root 保留 capabilities。请只运行可信的 rootfs 和命令；
安全选项不会将其变成可承载恶意工作负载的沙箱。
