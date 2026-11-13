# Migrate to one JSON configuration file

All container settings now live in `$HOME/.kurumi-containerd/config.json`.
The old `name/file` pointers and standalone TOML loading are no longer supported.
CLI `-c/--config` and `KURUMI_CONTAINERD_CONFIG` remain unsupported.

1. Stop existing containers before migration. Save copies of the old JSON and
   TOML files so you can roll back with the old binary.
2. Convert each TOML into a JSON object with the same `runtime` and `container`
   groups, then put those objects in the JSON list. Python 3.11 or newer can
   convert a single TOML without changing the original:

```sh
python3 - /srv/kurumi/debian/kurumi-containerd.toml > /tmp/kurumi-config.json <<'PYTHON'
import json, sys, tomllib
with open(sys.argv[1], "rb") as source:
    print(json.dumps([tomllib.load(source)], indent=2))
PYTHON
```

3. Preserve each original `container.name` and UUID. Remove the outer management
   `name` and `file` fields. Use `container.name` for `--name`, backup IDs, TUI,
   and Android `autostart.txt`. If names differed, update scripts and autostart
   to the original `container.name`. Names must be unique across the list.
4. Convert relative `rootfs`, `rootfs_image`, mount source, and `environment_file`
   paths to absolute paths based on their original TOML directory. They will
   otherwise resolve from the directory containing `config.json`. Init and
   mount targets remain container paths.
5. Review the converted file, then save it under the correct HOME. With sudo,
   use `sudo -H` consistently when editing and running the program:

```sh
sudo -H sh -c 'mkdir -p "$HOME/.kurumi-containerd"; exec vi "$HOME/.kurumi-containerd/config.json"'
```

The resulting file looks like this (keep your actual UUID and settings):

```json
[
  {
    "runtime": {"stop_timeout_seconds": 15},
    "container": {"name": "debian", "rootfs": "/srv/kurumi/debian/rootfs", "init": "/sbin/init"}
  }
]
```

Use `--name NAME` before the command when the list contains multiple containers.
Verify with `sudo -H kurumi-containerd --name debian info` before starting.
`check` only verifies the host. Missing UUIDs are generated on normal runtime
loading and atomically saved in the selected entry; existing UUIDs are retained.

The runtime state format is unchanged. To roll back, restore the old binary and
saved configuration files together. Rootfs backups do not include `config.json`;
save it separately.
