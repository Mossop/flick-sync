#!/bin/sh
# Builds opkg packages (.ipk) for OpenWrt from a prebuilt flick-sync binary:
#
#   flick-sync           The server, a procd init script and UCI config.
#   luci-app-flick-sync  A LuCI page for configuring the server.
#
# Usage: build-ipk.sh <binary> [arch] [output-dir]
#
#   binary      Path to a statically linked flick-sync binary for the target.
#   arch        The opkg architecture string. Defaults to aarch64_cortex-a53,
#               which suits MediaTek MT798x devices such as the GL.iNet
#               GL-BE10000. Check with `opkg print-architecture` on the device.
#   output-dir  Where to write the packages. Defaults to target/openwrt.
set -eu

BINARY=${1:?usage: build-ipk.sh <binary> [arch] [output-dir]}
ARCH=${2:-aarch64_cortex-a53}
OUT_DIR=${3:-target/openwrt}

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/packages/cli/Cargo.toml" | head -n 1)

[ -f "$BINARY" ] || { echo "Binary not found: $BINARY" >&2; exit 1; }
[ -n "$VERSION" ] || { echo "Could not determine version" >&2; exit 1; }

mkdir -p "$OUT_DIR"
OUT_DIR=$(cd "$OUT_DIR" && pwd)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Produce tarballs owned by root regardless of who runs the build.
tar_root() {
	if tar --version 2>/dev/null | grep -q GNU; then
		tar --owner=0 --group=0 --numeric-owner "$@"
	else
		tar --uid 0 --gid 0 --uname root --gname root "$@"
	fi
}

# Default maintainer scripts. These call OpenWrt's handlers, which enable and
# start init scripts on install, stop and disable them on removal, and reload
# rpcd when ACLs change.
write_default_scripts() {
	cat > "$1/postinst" <<'SCRIPT'
#!/bin/sh
[ -n "${IPKG_INSTROOT}" ] || {
	. /lib/functions.sh
	default_postinst "$0" "$@"
}
SCRIPT
	cat > "$1/prerm" <<'SCRIPT'
#!/bin/sh
[ -n "${IPKG_INSTROOT}" ] || {
	. /lib/functions.sh
	default_prerm "$0" "$@"
}
SCRIPT
	chmod 755 "$1/postinst" "$1/prerm"
}

# build_package <name> <arch> <control-fields-file>
#
# Expects $WORK/<name>/data to contain the files to install and
# $WORK/<name>/control to contain any extra control files (conffiles,
# postinst, prerm). Writes the package to $OUT_DIR.
build_package() {
	local name=$1 arch=$2 fields=$3
	local dir="$WORK/$name"
	local size

	size=$(du -sk "$dir/data" | cut -f 1)

	{
		echo "Package: $name"
		echo "Version: $VERSION"
		echo "Architecture: $arch"
		echo "Installed-Size: $((size * 1024))"
		echo "Maintainer: David Townsend"
		echo "Source: https://github.com/Mossop/flick-sync"
		echo "License: MIT"
		cat "$fields"
	} > "$dir/control/control"

	echo "2.0" > "$dir/debian-binary"
	(cd "$dir/control" && tar_root -czf "$dir/control.tar.gz" .)
	(cd "$dir/data" && tar_root -czf "$dir/data.tar.gz" .)

	local ipk="$OUT_DIR/${name}_${VERSION}_${arch}.ipk"
	(cd "$dir" && tar_root -czf "$ipk.tmp" ./debian-binary ./control.tar.gz ./data.tar.gz)
	mv "$ipk.tmp" "$ipk"
	echo "Built $ipk"
}

# flick-sync: the server itself.
PKG="$WORK/flick-sync"
mkdir -p "$PKG/control" "$PKG/data/usr/bin"
cp -R "$ROOT/openwrt/flick-sync/files/." "$PKG/data/"
cp "$BINARY" "$PKG/data/usr/bin/flick-sync"
chmod 755 "$PKG/data/usr/bin/flick-sync" "$PKG/data/etc/init.d/flick-sync" \
	"$PKG/data/usr/libexec/flick-sync/start"
chmod 644 "$PKG/data/etc/config/flick-sync" "$PKG/data/etc/hotplug.d/block/90-flick-sync"
echo "/etc/config/flick-sync" > "$PKG/control/conffiles"
write_default_scripts "$PKG/control"
cat > "$WORK/flick-sync.fields" <<FIELDS
Section: multimedia
Priority: optional
Description: Synchronises media from a Plex Media Server to local storage
 and serves it over DLNA and a web interface.
FIELDS
build_package flick-sync "$ARCH" "$WORK/flick-sync.fields"

# luci-app-flick-sync: the LuCI configuration page.
PKG="$WORK/luci-app-flick-sync"
mkdir -p "$PKG/control" "$PKG/data"
cp -R "$ROOT/openwrt/luci-app-flick-sync/files/." "$PKG/data/"
find "$PKG/data" -type d -exec chmod 755 {} +
find "$PKG/data" -type f -exec chmod 644 {} +
write_default_scripts "$PKG/control"
# LuCI caches its menu, so clear it after installing or removing the page,
# and signal rpcd to pick up the new ACL. This mirrors what OpenWrt's luci.mk
# does for its own packages.
for script in postinst prerm; do
	cat >> "$PKG/control/$script" <<'SCRIPT'
[ -n "${IPKG_INSTROOT}" ] || {
	rm -f /tmp/luci-indexcache /tmp/luci-indexcache.*
	rm -rf /tmp/luci-modulecache
	killall -HUP rpcd 2>/dev/null
}
exit 0
SCRIPT
done
cat > "$WORK/luci-app-flick-sync.fields" <<FIELDS
Section: luci
Priority: optional
Depends: luci-base, flick-sync
Description: LuCI page for configuring the Flick Sync server.
FIELDS
build_package luci-app-flick-sync all "$WORK/luci-app-flick-sync.fields"
