#!/bin/sh
# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

set -eu
[ "$#" -eq 2 ] || { echo "usage: $0 ROOTFS PINNED_SERVICECTL_SOURCE" >&2; exit 2; }
root=$(realpath "$1")
source=$(realpath "$2")
assets=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
[ "$root" != / ] || { echo "refusing host root" >&2; exit 1; }
[ "$(git -C "$source" rev-parse HEAD)" = 9bc7e99a4406a62b5dd745dac2f45472f6ed0b9a ] || {
    echo "servicectl source must be at the documented pinned commit" >&2; exit 1;
}
[ -z "$(git -C "$source" status --porcelain)" ] || { echo "servicectl source must be clean" >&2; exit 1; }
install -d "$root/usr/local/bin" "$root/usr/local/sbin" "$root/etc/dinit.d" \
    "$root/etc/systemd/system" "$root/s6/rc" "$root/var/lib/kurumi-demo"
# Build outside Kurumi; Go is a deployment-time tool, never a Rust dependency.
(cd "$source" && CGO_ENABLED=0 go build -trimpath -o "$root/usr/local/bin/servicectl" .)
for helper in sys-notifyd sys-cgroupd sys-dbusd sys-logd sys-propertyd sysvisiond sys-orchestrd; do
    output=$helper
    [ "$helper" != sys-cgroupd ] || output=sys-cgroupd.real
    (cd "$source" && CGO_ENABLED=0 go build -trimpath -o "$root/usr/local/bin/$output" "./cmd/$helper")
done
install -m 0755 "$assets/sys-cgroupd" "$root/usr/local/bin/sys-cgroupd"
install -m 0755 "$assets/kurumi-services" "$root/usr/local/sbin/kurumi-services"
install -m 0755 "$assets/kurumi-demo" "$root/usr/local/bin/kurumi-demo"
install -m 0644 "$assets/kurumi-anchor" "$root/etc/dinit.d/kurumi-anchor"
install -m 0644 "$assets/kurumi-demo.service" "$root/etc/systemd/system/kurumi-demo.service"
printf '%s\n' 'Installed. Rootfs still requires Python 3, Dinit protocol >=5, s6, s6-rc and execline.'
