# Configuration Entry Point Migration Guide (Breaking Change)

> This guide applies to versions that use the HOME JSON configuration pointer. For older versions, continue using the original configuration arguments.

## What changed

The configuration entry point is now `$HOME/.kurumi-containerd/config.json`:

| Previous entry point | Migration |
| --- | --- |
| `-c /path/config.toml` | Put the original path in the JSON `file` field |
| `--config /path/config.toml` | Put the original path in the JSON `file` field |
| `KURUMI_CONTAINERD_CONFIG` | Put its value in `file` and remove the old environment variable setting |
| Default `kurumi-containerd.toml` in the working directory | Put that file's absolute path in `file` |

After upgrading, `-c/--config` produces an argument error. The old environment variable and the default file in the working directory no longer select the configuration.

## 1. Keep your existing TOML

You do not need to convert TOML to JSON or move your rootfs or TOML. Note the absolute path to your existing TOML, for example:

```text
/srv/kurumi/debian/kurumi-containerd.toml
```

Keep the existing `container.name`, UUID, and other settings in the TOML. Relative paths inside TOML still resolve from its directory. Leave the TOML in place so those paths retain their meaning.

## 2. Create the pointer under the correct HOME

The program uses the running process's `HOME`. If you normally use sudo, use `sudo -H` consistently so pointer creation and program execution use the same target user's HOME. Do not create the file under your regular user's HOME and assume sudo will read it.

On Linux with sudo:

```bash
sudo -H sh -c 'mkdir -p "$HOME/.kurumi-containerd"; printf "%s\n" "$HOME/.kurumi-containerd/config.json"'
sudo -H sh -c 'exec vi "$HOME/.kurumi-containerd/config.json"'
```

If you are already in a root shell on Linux or Android, first confirm that `HOME` is the intended absolute path:

```sh
printf '%s\n' "$HOME"
mkdir -p "$HOME/.kurumi-containerd"
vi "$HOME/.kurumi-containerd/config.json"
```

If the JSON already exists, back it up and inspect its contents before editing. Replace the path below with the actual absolute path to your original TOML:

```json
[
  {
    "name": "debian",
    "file": "/srv/kurumi/debian/kurumi-containerd.toml"
  }
]
```

- The top level must be a nonempty list. The outer `[]` is required even for one configuration.
- `name` is the management display and selection name. It must be unique, with exact, case-sensitive matching. It does not override TOML `container.name` or rename the container.
- Both `name` and `file` must be nonblank strings. Additional fields are not supported.
- Absolute paths are recommended. Relative paths resolve from the `.kurumi-containerd` directory containing the JSON.
- `file` does not expand `~` or `$HOME`; enter the actual expanded path.

## 3. Update commands and scripts

After upgrading to a version containing this change, replace:

```bash
sudo kurumi-containerd -c /srv/kurumi/debian/kurumi-containerd.toml start
```

with:

```bash
sudo -H kurumi-containerd start
```

Remove the configuration arguments from `install`, `stop`, `restart`, `enter`, `run`, `info`, `pid`, `show`, and `scan` as well. Update shell aliases, startup scripts, and service configurations so their HOME points to the directory containing your JSON. Remove any `KURUMI_CONTAINERD_CONFIG` settings.

If you previously switched containers using different `-c` paths, add each configuration to the list:

```json
[
  {"name": "debian", "file": "/srv/kurumi/debian/kurumi-containerd.toml"},
  {"name": "alpine", "file": "/srv/kurumi/alpine/kurumi-containerd.toml"}
]
```

For a single-entry list, the selection argument is optional. For multiple entries, provide `--name` before the subcommand. The program will not default to the first entry:

```bash
sudo -H kurumi-containerd --name debian start
sudo -H kurumi-containerd --name alpine info
```

Also add `--name NAME` to the verification commands below when using multiple entries.
If you previously created a single-object JSON, wrap that object in `[` and `]` to migrate it to a single-entry list.

## 4. Verify the migration

For an existing running container:

```bash
sudo -H kurumi-containerd info
```

Confirm that the output identifies your original container. If the container is not running, execute the following when you are ready to start it:

```bash
sudo -H kurumi-containerd start
sudo -H kurumi-containerd info
```

`check` does not read container configuration, so a successful check does not verify the pointer migration. Missing or invalid configuration produces an error; the program does not automatically create configuration or fall back to old paths.

Troubleshooting:

| Problem | What to check |
| --- | --- |
| `config.json` not found | Check the running process's HOME, especially under sudo or a service |
| JSON parsing or field validation fails | Use double quotes, remove comments, trailing commas, and unknown fields, and supply `name` and `file` |
| TOML not found | Set `file` to the original TOML's absolute path |
| Rootfs or mount paths in TOML no longer work | Confirm the TOML has not moved and its relative paths still refer to the original resources |
| `-c/--config` rejected | Remove leftover configuration arguments from scripts or aliases |

## Rollback

Keep the old binary and original TOML. To roll back, restore the old version and original command, such as `kurumi-containerd -c /srv/kurumi/debian/kurumi-containerd.toml info`. You can leave the new JSON in place; older versions do not read it.

The pointer itself does not convert TOML or change the runtime state format. Normal configuration loading still automatically persists a UUID when one is missing.
