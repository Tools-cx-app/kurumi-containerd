# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

# Runs inside the selected container. Each section contains the tool's native output.
set -e
export LC_ALL=C SYSTEMD_COLORS=0
if [ -d /run/systemd/system ] && command -v systemctl >/dev/null; then
    echo '[systemd files]'
    systemctl --system --no-pager --no-legend --full list-unit-files --type=service
    echo '[systemd units]'
    systemctl --system --no-pager --no-legend --full --plain list-units --all --type=service
elif command -v rc-service >/dev/null && command -v rc-status >/dev/null && command -v rc-update >/dev/null; then
    echo '[openrc services]'
    rc-service --list
    echo '[openrc status]'
    rc-status --all --nocolor
    echo '[openrc default]'
    rc-update show default
else
    echo 'No systemd or OpenRC service manager found in this container.' >&2
    exit 1
fi
echo '[end services]'
