#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

# Run from the repository root; TARGET, VERSION and PLATFORM come from the workflow.
: "${TARGET:?}" "${VERSION:?}" "${PLATFORM:?}"
binary="target/$TARGET/release/kurumi-containerd"
name="kurumi-containerd-$VERSION-$PLATFORM-$TARGET"
mkdir -p dist
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
archive="$work/$name"
mkdir -p "$archive"
install -m 0755 "$binary" "$archive/kurumi-containerd"
install -m 0644 LICENSE README.md README_CN.md kurumi-containerd.example.toml "$archive/"
tar --owner=0 --group=0 -C "$work" -cJf "dist/$name.tar.xz" "$name"
tar -tJf "dist/$name.tar.xz" | grep -Fx "$name/kurumi-containerd"

[[ "$PLATFORM" == linux && "$TARGET" == *-gnu* ]] || exit 0
case "$TARGET" in
  x86_64-unknown-linux-gnu) deb_arch=amd64; rpm_arch=x86_64 ;;
  aarch64-unknown-linux-gnu) deb_arch=arm64; rpm_arch=aarch64 ;;
  armv7-unknown-linux-gnueabihf) deb_arch=armhf; rpm_arch=armv7hl ;;
  riscv64gc-unknown-linux-gnu) deb_arch=riscv64; rpm_arch=riscv64 ;;
  *) echo "Unsupported package target: $TARGET" >&2; exit 1 ;;
esac

# Debian sorts prereleases before the final version; RPM uses the same convention.
package_version=${VERSION/-/\~}
package_version=${package_version//-/.}
payload="$work/payload"
mkdir -p "$payload/usr/bin" "$payload/usr/share/doc/kurumi-containerd"
install -m 0755 "$binary" "$payload/usr/bin/kurumi-containerd"
install -m 0644 LICENSE README.md README_CN.md kurumi-containerd.example.toml \
  "$payload/usr/share/doc/kurumi-containerd/"

# Read the target ELF rather than executing a cross-built binary on the runner.
glibc=$(readelf --version-info "$binary" | grep -oE 'GLIBC_[0-9]+(\.[0-9]+)+' | sort -Vu | tail -n 1)
depends="libc6 (>= ${glibc#GLIBC_})"
needed=$(readelf -d "$binary" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p')
while IFS= read -r library; do
  case "$library" in
    libc.so.6|libm.so.6|libpthread.so.0|libdl.so.2|librt.so.1|libutil.so.1|ld-linux*.so.*) ;;
    libgcc_s.so.1) depends+=", libgcc-s1" ;;
    liblzma.so.5) depends+=", liblzma5" ;;
    *) echo "Unmapped Debian dependency: $library" >&2; exit 1 ;;
  esac
done <<< "$needed"

deb="$work/deb"
cp -a "$payload" "$deb"
mkdir -p "$deb/DEBIAN"
cat > "$deb/DEBIAN/control" <<EOF
Package: kurumi-containerd
Version: $package_version
Section: admin
Priority: optional
Architecture: $deb_arch
Maintainer: KurumiContainerd contributors <noreply@kurumi-containerd.invalid>
Depends: $depends
Description: Privileged Linux container runtime and command-line tool
EOF
dpkg-deb --build --root-owner-group "$deb" "dist/$name.deb"
test "$(dpkg-deb -f "dist/$name.deb" Architecture)" = "$deb_arch"
test "$(dpkg-deb -f "dist/$name.deb" Version)" = "$package_version"

rpm_top="$work/rpm"
mkdir -p "$rpm_top"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
cp -a "$payload" "$rpm_top/SOURCES/payload"
cat > "$rpm_top/SPECS/kurumi-containerd.spec" <<EOF
Name: kurumi-containerd
Version: $package_version
Release: 1
Summary: Privileged Linux container runtime and command-line tool
License: GPL-3.0-only

%description
Privileged Linux container runtime and command-line tool.

%prep
%build
%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/payload/. %{buildroot}/

%files
%attr(0755,root,root) /usr/bin/kurumi-containerd
%dir %attr(0755,root,root) /usr/share/doc/kurumi-containerd
%license %attr(0644,root,root) /usr/share/doc/kurumi-containerd/LICENSE
%doc %attr(0644,root,root) /usr/share/doc/kurumi-containerd/README.md
%doc %attr(0644,root,root) /usr/share/doc/kurumi-containerd/README_CN.md
%doc %attr(0644,root,root) /usr/share/doc/kurumi-containerd/kurumi-containerd.example.toml
EOF
# Cargo already strips the target binary. Host strip/debug tools cannot process every target.
rpmbuild --target "$rpm_arch" --define "_topdir $rpm_top" \
  --define '_build_id_links none' --define 'debug_package %{nil}' \
  --define '__os_install_post %{nil}' -bb "$rpm_top/SPECS/kurumi-containerd.spec"
rpms=("$rpm_top"/RPMS/"$rpm_arch"/*.rpm)
test "${#rpms[@]}" -eq 1
cp "${rpms[0]}" "dist/$name.rpm"
test "$(rpm -qp --qf '%{ARCH}' "dist/$name.rpm")" = "$rpm_arch"
test "$(rpm -qp --qf '%{VERSION}' "dist/$name.rpm")" = "$package_version"
