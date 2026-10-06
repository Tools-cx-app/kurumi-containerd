#!/system/bin/sh
# Sourced by the manager after default extraction and permission setup.
[ "$BOOTMODE" = true ] || abort "Install from Magisk, KernelSU or APatch Manager."
[ "$API" -ge 21 ] || abort "Android API 21 or newer is required."
case "$ARCH" in
  arm64) abi=arm64-v8a ;;
  arm) abi=armeabi-v7a ;;
  x64) abi=x86_64 ;;
  x86) abi=x86 ;;
  *) abort "Unsupported architecture: $ARCH" ;;
esac
if [ -d "$MODPATH/binaries" ]; then
  [ -f "$MODPATH/binaries/$abi/kurumi-containerd" ] || abort "Missing binary: $abi"
  mv "$MODPATH/binaries/$abi/kurumi-containerd" "$MODPATH/bin/kurumi-containerd.bin" || abort "Cannot install binary."
  rm -rf "$MODPATH/binaries"
fi
[ -f "$MODPATH/bin/kurumi-containerd.bin" ] || abort "Missing runtime binary."
set_perm "$MODPATH/bin/kurumi-containerd.bin" 0 0 0755
set_perm "$MODPATH/bin/kurumi-containerd" 0 0 0755
set_perm "$MODPATH/system/bin/kurumi-containerd" 0 0 0755
set_perm "$MODPATH/service.sh" 0 0 0755
data=${KURUMI_CONTAINERD_HOME:-/data/adb/kurumi-containerd}
umask 077
[ ! -L "$data" ] || abort "Persistent directory must not be a symlink."
mkdir -p "$data/.kurumi-containerd" || abort "Cannot create persistent directory."
set_perm "$data" 0 0 0700
set_perm "$data/.kurumi-containerd" 0 0 0700
if [ ! -e "$data/autostart.txt" ]; then
  printf '# One registered container name per line. Use foreground = false.\n' > "$data/autostart.txt"
fi
ui_print "Installed $abi. Persistent HOME: $data"
ui_print "Configure .kurumi-containerd/config.json and autostart.txt under that HOME."
