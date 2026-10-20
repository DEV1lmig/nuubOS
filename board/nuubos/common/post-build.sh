#!/bin/sh
set -eu

TARGET_DIR="$1"
INITTAB="$TARGET_DIR/etc/inittab"

#
# nuubOS SYSTEM carries no persistent per-device/user state by policy.
#
# During development the filesystem may remain writable, but persistent
# configuration belongs in STATE and must never be embedded in SYSTEM.
#

mkdir -p \
    "$TARGET_DIR/etc/network" \
    "$TARGET_DIR/etc/wpa_supplicant" \
    "$TARGET_DIR/root/.ssh"

chmod 700 "$TARGET_DIR/root/.ssh"

#
# Production-safe networking baseline.
# Wi-Fi is enabled only after STATE has been provisioned.
#
cat > "$TARGET_DIR/etc/network/interfaces" <<'NETWORK_EOF'
auto lo
iface lo inet loopback
NETWORK_EOF

#
# Never ship credentials or user authorization in SYSTEM.
#
rm -f \
    "$TARGET_DIR/etc/wpa_supplicant/wpa_supplicant.conf" \
    "$TARGET_DIR/root/.ssh/authorized_keys" \
    "$TARGET_DIR/run/resolv.conf"

#
# Files of removed features: an incremental build never deletes what an
# older package version installed in TARGET_DIR (found before the first
# nightly, 2026-10-16). Per-profile backup (removed 2026-10-12, console
# backup replaces it), lifecycle-display (screensaver, removed 2026-10-10).
rm -f \
    "$TARGET_DIR/usr/bin/nuubos-backupctl" \
    "$TARGET_DIR/usr/share/nuubos/jobs/backup-user.job" \
    "$TARGET_DIR/usr/share/nuubos/jobs/restore-user.job" \
    "$TARGET_DIR/usr/lib/nuubos/lifecycle-display"

#
# Modules of earlier kernel versions stay in TARGET_DIR after a kernel bump
# (2026-10-20: 7.2.8 and 7.3.0-rc7 next to 7.3.0, 24 MB): keep only the
# release of the kernel this build configures.
if [ -n "${BR2_CONFIG:-}" ] && [ -n "${BUILD_DIR:-}" ]; then
    KVER="$(sed -n 's/^BR2_LINUX_KERNEL_CUSTOM_VERSION_VALUE="\(.*\)"$/\1/p' "$BR2_CONFIG")"
    KREL="$(cat "$BUILD_DIR/linux-$KVER/include/config/kernel.release" 2>/dev/null)"
    if [ -n "$KREL" ] && [ -d "$TARGET_DIR/lib/modules/$KREL" ]; then
        for d in "$TARGET_DIR"/lib/modules/*; do
            [ "${d##*/}" = "$KREL" ] || rm -rf "$d"
        done
    fi
fi

#
# Product release boot keeps tty1 free of getty output.
if [ -f "$INITTAB" ]; then
    sed -i '/^tty1::respawn:/d' "$INITTAB"
fi

echo "nuubOS rootfs configured with persistent state externalized"

#
# Product identity (EPIC-048 update checks, EPIC-051/052 build identifiers).
#
NUUBOS_TOP="${BR2_EXTERNAL_NUUBOS_PATH:-$(dirname "$0")/../../..}"
# Version (docs/technical/releasing.md): `VERSION` holds the release in
# preparation, X.Y.Z or X.Y.Z-rc.N; the image's channel (installed by
# nuubos-update) decides the published form: stable X.Y.Z, rc X.Y.Z-rc.N,
# nightly X.Y.Z-nightly.YYYYMMDD (UTC date of this build).
NUUBOS_SERIAL="$(date +%s)"
NUUBOS_CHANNEL="$(cat "$TARGET_DIR/usr/share/nuubos/release-channel" 2>/dev/null || echo nightly)"
NUUBOS_BASE="$(tr -d ' \n' < "$NUUBOS_TOP/VERSION")"
case "$NUUBOS_CHANNEL:$NUUBOS_BASE" in
    stable:[0-9]*.[0-9]*.[0-9]*-*|rc:[0-9]*.[0-9]*.[0-9]*-rc.*-*)
        echo "VERSION $NUUBOS_BASE does not fit the $NUUBOS_CHANNEL channel" >&2; exit 1 ;;
    stable:[0-9]*.[0-9]*.[0-9]*) NUUBOS_VERSION="$NUUBOS_BASE" ;;
    rc:[0-9]*.[0-9]*.[0-9]*-rc.[1-9]*) NUUBOS_VERSION="$NUUBOS_BASE" ;;
    nightly:[0-9]*.[0-9]*.[0-9]*)
        NUUBOS_VERSION="${NUUBOS_BASE%%-*}-nightly.$(date -u -d "@$NUUBOS_SERIAL" +%Y%m%d)" ;;
    *) echo "VERSION $NUUBOS_BASE does not fit the $NUUBOS_CHANNEL channel" >&2; exit 1 ;;
esac
NUUBOS_BUILD="$(git -C "$NUUBOS_TOP" describe --always --dirty --abbrev=12 2>/dev/null || echo unknown)"
# Buildroot's /etc/os-release is a link to /usr/lib/os-release: replace both.
rm -f "$TARGET_DIR/etc/os-release" "$TARGET_DIR/usr/lib/os-release"
cat > "$TARGET_DIR/etc/os-release" <<OS_RELEASE_EOF
NAME=nuubOS
ID=nuubos
VERSION_ID=$NUUBOS_VERSION
BUILD_ID=$NUUBOS_BUILD
PRETTY_NAME="nuubOS $NUUBOS_VERSION"
HOME_URL="https://github.com/baf93/nuubOS"
OS_RELEASE_EOF
ln -s ../../etc/os-release "$TARGET_DIR/usr/lib/os-release"
mkdir -p "$TARGET_DIR/usr/share/nuubos"
printf '%s\n' "$NUUBOS_BUILD" > "$TARGET_DIR/usr/share/nuubos/build-id"
# OTA order (EPIC-048): a release is offered only above the running serial.
echo "$NUUBOS_SERIAL" > "$TARGET_DIR/usr/share/nuubos/build-serial"
