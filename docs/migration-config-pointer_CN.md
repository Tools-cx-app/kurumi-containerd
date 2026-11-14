# 迁移到单一 JSON 配置文件

[[zh](migration-config-pointer_CN.md) / [en](migration-config-pointer.md)]

[文档目录](README_CN.md)

## 变更内容

全部容器设置保存在 `$HOME/.kurumi-containerd/config.json` 中，顶层是由完整配置对象
组成的非空列表。不再支持旧的外层 `name`/`file` 指针或独立 TOML 加载。
CLI `-c/--config` 和 `KURUMI_CONTAINERD_CONFIG` 也不受支持。

`container.name` 统一用于 CLI 选择、备份、TUI 和 Android 自启动。
运行时状态格式保持不变。

## 转换现有设置

1. 使用旧二进制和配置停止已有容器。备份旧 JSON 和 TOML 文件，以便与旧二进制一起恢复。
2. 将每份 TOML 转为保留 `runtime`、`container` 分组的对象，再将所有对象合并为一个
   JSON 列表。Python 3.11 或更高版本可以转换单份 TOML，不修改源文件：

```sh
python3 - /srv/kurumi/debian/kurumi-containerd.toml > /tmp/kurumi-config.json <<'PYTHON'
import json, sys, tomllib
with open(sys.argv[1], "rb") as source:
    print(json.dumps([tomllib.load(source)], indent=2))
PYTHON
```

3. 保留原有 `container.name` 和 UUID，删除外层管理用的 `name`/`file` 字段。
   如果旧管理名称与 `container.name` 不同，请同步修改 CLI 脚本、备份 ID 和 Android
   `autostart.txt`，统一使用 `container.name`。新列表中的名称不能重复。
4. 根据各份旧 TOML 所在目录，将相对 `rootfs`、`rootfs_image`、挂载 `source` 和
   `environment_file` 转为主机绝对路径；否则它们会以新 `config.json` 所在目录为基准解析。
   init 和挂载目标仍然使用容器内路径。
5. 按[配置参考](configuration_CN.md)检查所有转换后的对象，将合并列表保存到运行 CLI
   所使用的 HOME。使用 root 时，编辑和运行都应一致地使用 `sudo -H`：

```sh
sudo -H sh -c 'mkdir -p "$HOME/.kurumi-containerd"; exec vi "$HOME/.kurumi-containerd/config.json"'
```

结果结构如下；请保留实际 UUID 和设置：

```json
[
  {
    "runtime": {"stop_timeout_seconds": 15},
    "container": {"name": "debian", "rootfs": "/srv/kurumi/debian/rootfs", "init": "/sbin/init"}
  }
]
```

## 验证与回滚

列表包含多个条目时，将 `--name NAME` 写在子命令前：

```sh
sudo -H kurumi-containerd --name debian info
sudo -H kurumi-containerd check
```

`info` 加载并校验选中配置；容器停止时仍可能报告容器未运行。
`check` 只检查主机能力。普通运行时加载会原子补写选中条目缺失的 UUID，已有 UUID
保持不变。安装、备份和 TUI 的只读刷新不会补写 UUID。

回滚时应同时恢复旧二进制和备份的配置文件。rootfs 备份不包含 `config.json`，
请单独保存。此次迁移不转换 rootfs 数据，也不改变运行时状态格式。
