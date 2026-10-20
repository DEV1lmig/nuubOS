#!/bin/sh
# Add the flashable image and the license material to a release made by
# tools/ota/make-update.sh, so tools/ota/publish.sh uploads them with the
# OTA payload (EPIC-048, LICENSING.md, docs/dev/build-deploy.md).
#
#   tools/ota/add-image.sh OUTDIR
#
# Needs a full image build and `compile.sh legal-info` of the same build.
# Writes into OUTDIR/<tag>/:
#   nuubOS-<version>-anbernic-h700.img.xz     image for SD1
#   nuubOS-<version>-legal-info.tar.partNN    legal-info, split
#                                                      below GitHub's 2 GiB
#   nuubOS-<version>-third-party-notices.tar.gz
#   SHA256SUMS                                         every file above + payload

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-}"
[ -f "$OUT/release.env" ] || { echo "Usage: $0 OUTDIR (made by tools/ota/make-update.sh)" >&2; exit 2; }
. "$OUT/release.env"
IMAGES="$ROOT/output/h700/images"
LEGAL="$ROOT/output/h700/legal-info"
IMG="$IMAGES/nuubOS-anbernic-h700.img"
TARGET="$ROOT/output/h700/target"
[ -f "$IMG" ] || { echo "[FAIL] $IMG missing: run a full image build" >&2; exit 1; }
[ -f "$LEGAL/manifest.csv" ] || { echo "[FAIL] $LEGAL missing: run compile.sh legal-info" >&2; exit 1; }
# legal-info must describe this build.
[ "$LEGAL/manifest.csv" -nt "$IMAGES/rootfs.tar" ] || {
    echo "[FAIL] legal-info is older than the image: run compile.sh legal-info again" >&2; exit 1; }
BASE="nuubOS-$VERSION"
DIR="$OUT/$TAG"
mkdir -p "$DIR" || exit 1

echo "Compressing the image..."
xz -T0 -9 -c "$IMG" > "$DIR/$BASE-anbernic-h700.img.xz.tmp" && \
    mv "$DIR/$BASE-anbernic-h700.img.xz.tmp" "$DIR/$BASE-anbernic-h700.img.xz" || exit 1

echo "Packing legal-info..."
rm -f "$DIR/$BASE-legal-info.tar.part"*
tar -cf - -C "$(dirname "$LEGAL")" "$(basename "$LEGAL")" -C "$ROOT" LICENSE LICENSING.md licenses |
    split -b 1900M -d -a 2 - "$DIR/$BASE-legal-info.tar.part" || exit 1

echo "Packing third-party notices..."
tar -C "$TARGET/usr/share" -czf "$DIR/$BASE-third-party-notices.tar.gz" licenses || exit 1

( cd "$DIR" && sha256sum -- * | grep -v ' SHA256SUMS$' > SHA256SUMS ) || exit 1
echo "[PASS] $DIR: image, legal-info ($(ls "$DIR/$BASE-legal-info.tar.part"* | wc -l) parts), notices, SHA256SUMS"
