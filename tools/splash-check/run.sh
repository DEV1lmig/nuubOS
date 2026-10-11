#!/bin/sh
# Host check: the C splash renderer (kernel fbdev + labwc first frame) and
# the Rust port (nuubUI, Quick Menu) give identical pixels.
#   sh tools/splash-check/run.sh
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "${T:?}"' EXIT
RUSTC="${RUSTC:-$(command -v rustc || echo "$ROOT/output/h700/host/bin/rustc")}"
FAIL=0
cc -O2 -Wall -Wextra -Werror -D_GNU_SOURCE $(pkg-config --cflags libdrm) -o "$T/c" "$ROOT/package/nuubos/nuubos-splash/src/nuubos-splash.c" $(pkg-config --libs libdrm) || exit 1
"$RUSTC" -O --edition 2021 -o "$T/r" "$ROOT/tools/splash-check/main.rs" || exit 1
cmp -s "$ROOT/package/nuubos/nuubui-home/src/src/splash_core.rs" "$ROOT/package/nuubos/nuubos-quick-menu/src/src/splash_core.rs" ||
    { echo "[FAIL] nuubui-home and nuubos-quick-menu splash_core.rs differ"; FAIL=1; }
printf 'background=#0e0f0c\ntext=#f3f6ef\naccent=#95cf4a\nsurface=#272a23\n' > "$T/pal"
for size in 640x480 720x720 1280x720 1920x1080 1024x600 480x320; do
    w=${size%x*}; h=${size#*x}
    for mode in boot reboot poweroff full; do
        for pal in "" "$T/pal"; do
            "$T/c" --ppm "$T/c.ppm" "$w" "$h" "$mode" $pal
            "$T/r" "$T/r.ppm" "$w" "$h" "$mode" $pal
            cmp -s "$T/c.ppm" "$T/r.ppm" || { echo "[FAIL] $size $mode ${pal:+mood}"; FAIL=1; }
        done
    done
done
[ "$FAIL" = 0 ] && echo "[PASS] splash renderers identical"
exit "$FAIL"
