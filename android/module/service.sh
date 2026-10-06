#!/system/bin/sh
MODDIR=${0%/*}
data=${KURUMI_CONTAINERD_HOME:-/data/adb/kurumi-containerd}
umask 077
[ -d "$data" ] && [ ! -L "$data" ] || exit 1
[ ! -f "$MODDIR/disable" ] && [ ! -f "$MODDIR/remove" ] || exit 0
[ -f "$data/autostart.txt" ] || exit 0
exec > "$data/boot.log" 2>&1
while [ "$(getprop sys.boot_completed)" != 1 ]; do
  [ ! -f "$MODDIR/disable" ] && [ ! -f "$MODDIR/remove" ] || exit 0
  sleep 2
done
# ponytail: one boot attempt per entry; add retry policy only for demonstrated device needs.
while IFS= read -r name || [ -n "$name" ]; do
  case "$name" in ''|'#'*) continue ;; esac
  [ ! -f "$MODDIR/disable" ] && [ ! -f "$MODDIR/remove" ] || exit 0
  if timeout 120 sh "$MODDIR/bin/kurumi-containerd" --name "$name" pid; then
    printf 'Already running: %s\n' "$name"
    continue
  fi
  printf 'Starting: %s\n' "$name"
  if ! timeout 120 sh "$MODDIR/bin/kurumi-containerd" --name "$name" start; then
    printf 'Startup failed: %s (check configuration and foreground = false)\n' "$name"
  fi
done < "$data/autostart.txt"
