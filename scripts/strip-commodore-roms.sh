#!/bin/sh
# Remove the Commodore (and CMD) ROM images from a vice-libretro source tree.
#
#   scripts/strip-commodore-roms.sh SRCDIR
#
# Upstream VICE carries the KERNAL/BASIC/character/drive/printer ROMs, both
# as files (vice/data/<machine>/) and compiled into the libretro core
# (include/embedded/*.h). Their copyright holder does not allow
# redistribution (Debian ships VICE in contrib without them), so nuubOS
# ships neither: the core loads them from the user's BIOS folder,
# /userdata/bios/vice/<machine>/ (bios.conf, LICENSING.md). VICE's own data
# (palettes, keymaps, symbols) stays. Used by libretro-vice.mk on the build
# tree and on the source archive saved by legal-info. Idempotent.

SRC="${1:-}"
[ -d "$SRC/include/embedded" ] && [ -d "$SRC/vice/data" ] || {
    echo "Usage: $0 VICE_LIBRETRO_SOURCE_DIR" >&2; exit 2; }

# Embedded copies: each ROM header becomes "#define <array> NULL" (palettes,
# *_vpl.h, are VICE's own and stay).
for h in "$SRC"/include/embedded/*.h; do
    case "$h" in *_vpl.h) continue ;; esac
    grep -q 'removed by nuubOS' "$h" && continue
    name="$(grep -o -m1 '[A-Za-z_][A-Za-z0-9_]*\[' "$h" | head -n1 | tr -d '[')"
    [ -n "$name" ] || { echo "[FAIL] no array in $h" >&2; exit 1; }
    printf '/* Commodore ROM removed by nuubOS (not redistributable): loaded from the BIOS folder. */\n#define %s NULL\n' "$name" > "$h"
done
# An entry without data must read as "not embedded" so sysfile_load() falls
# back to the file (upstream returns its size without copying anything).
for c in "$SRC"/retrodep/embedded.c "$SRC"/retrodep/embedded/*.c; do
    sed -i 's/if (emb\[i\]\.esrc != NULL) {/if (emb[i].esrc == NULL) { return 0; } {/' "$c"
done

# ROM image files: everything in the machine folders except VICE's own
# configuration files.
for d in C64 C128 C64DTV CBM-II DRIVES PET PLUS4 PRINTER SCPU64 VIC20; do
    [ -d "$SRC/vice/data/$d" ] || continue
    find "$SRC/vice/data/$d" -maxdepth 1 -type f \
        ! -name '*.vpl' ! -name '*.vkm' ! -name '*.vrs' ! -name '*.vjm' ! -name '*.vjk' \
        ! -name '*.sym' ! -name '*.txt' ! -name '*.md' ! -name '*.am' ! -name '*.in' \
        -exec rm -f {} +
done
exit 0
