# Migrate to one JSON configuration file

[[zh](migration-config-pointer_CN.md) / [en](migration-config-pointer.md)]

[Documentation](README.md)

## What changed

All container settings live in `$HOME/.kurumi-containerd/config.json` as a
nonempty list of complete configuration objects. The old outer `name`/`file`
pointers and standalone TOML loading are no longer supported. CLI
`-c/--config` and `KURUMI_CONTAINERD_CONFIG` are unsupported.

`container.name` is the shared name for CLI selection, backup, TUI and Android
autostart. Runtime state format is unchanged.

## Convert existing settings

1. Stop existing containers using the old binary and configuration. Save the
   old JSON and TOML files so you can restore them with that binary.
2. Convert each TOML file into an object retaining its `runtime` and `container`
   groups, and combine all objects into one JSON list. Python 3.11 or newer can
   convert a single TOML without changing the source:

```sh
python3 - /srv/kurumi/debian/kurumi-containerd.toml > /tmp/kurumi-config.json <<'PYTHON'
import json, sys, tomllib
with open(sys.argv[1], "rb") as source:
    print(json.dumps([tomllib.load(source)], indent=2))
PYTHON
```

3. Preserve the original `container.name` and UUID. Remove outer management
   `name`/`file` fields. If the old management name differed from
   `container.name`, update CLI scripts, backup IDs and Android `autostart.txt`
   to use `container.name`. Every name in the new list must be unique.
4. Convert relative `rootfs`, `rootfs_image`, mount `source` and
   `environment_file` paths to absolute host paths using each original TOML
   directory. Otherwise they resolve relative to the new `config.json` directory.
   Keep init and mount targets as container paths.
5. Review all converted objects against the [configuration reference](configuration.md).
   Save the combined list under the HOME used to run the CLI. Use `sudo -H`
   consistently when editing and running as root:

```sh
sudo -H sh -c 'mkdir -p "$HOME/.kurumi-containerd"; exec vi "$HOME/.kurumi-containerd/config.json"'
```

The result has this shape; retain your actual UUIDs and settings:

```json
[
  {
    "runtime": {"stop_timeout_seconds": 15},
    "container": {"name": "debian", "rootfs": "/srv/kurumi/debian/rootfs", "init": "/sbin/init"}
  }
]
```

## Verify and roll back

With multiple entries, put `--name NAME` before the subcommand:

```sh
sudo -H kurumi-containerd --name debian info
sudo -H kurumi-containerd check
```

`info` loads and validates the selected configuration; a stopped container can
still report that it is not running. `check` verifies only host capabilities.
Normal runtime loading atomically writes missing UUIDs to the selected entry;
existing UUIDs are retained. Installation, backup and read-only TUI refresh do
not persist missing UUIDs.

To roll back, restore the old binary and saved configuration files together.
Rootfs backups do not include `config.json`; save it separately. The migration
does not convert rootfs data or change runtime state format.
