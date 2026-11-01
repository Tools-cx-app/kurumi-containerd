#!/bin/sh
# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

set -eu

case "$1" in
    check)
        printf 'stdout\n'
        printf 'stderr\n' >&2
        printf '\377'
        head -c 1048576 /dev/zero
        printf 'not captured'
        ;;
    backup)
        while [ ! -f "$3" ]; do sleep 0.01; done
        printf done
        ;;
    *) exit 1 ;;
esac
