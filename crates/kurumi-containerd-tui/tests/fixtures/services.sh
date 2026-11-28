#!/bin/sh
# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only
set -eu
cd "${0%/*}"
case "${0##*/}" in
    cli)
        [ "$1" = --name ] && [ "$2" = alpine ] && [ "$3" = run ] || exit 41
        shift 3
        export PATH="$PWD"
        exec "$@"
        ;;
    rc-service) [ "$1" = --list ] && echo sshd ;;
    rc-status) echo ' sshd [ started ]' ;;
    rc-update)
        case "$1" in
            add) [ "$2" = sshd ] && [ "$3" = default ] && : > enabled ;;
            show) if [ -f enabled ]; then echo 'sshd | default'; fi ;;
            *) exit 42 ;;
        esac
        ;;
esac
