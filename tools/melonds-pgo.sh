#!/usr/bin/env bash
#
# Trains the profile of libretro-melonds (package/libretro/libretro-melonds/
# pgo/, docs/dev/emulation.md "melonDS PGO") on the development device.
#
# Usage:
#   1. build the instrumented core (Docker, as any targeted build):
#        NUUBOS_MELONDS_PGO=generate ./scripts/compile.sh \
#            libretro-melonds-dirclean libretro-melonds
#      (docker run ... -e NUUBOS_MELONDS_PGO=generate ...)
#   2. tools/melonds-pgo.sh [-H root@host] [-t seconds] GAME_ID:MODE ...
#        MODE = resume | new | slot:N (nuubos-emuctl launch)
#   3. rebuild the normal core with the new profile:
#        ./scripts/compile.sh libretro-melonds-dirclean libretro-melonds
#      and deploy it (tools/deploy-target.sh /usr/lib/libretro/melonds_nuubos_libretro.so).
#
# The instrumented core is bind-mounted over the installed one only while
# the games run (nothing is installed). Every quit writes a save state like a
# normal quit: the script backs up the user's NDS states and restores them.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE="$ROOT/output/h700/target/usr/lib/libretro/melonds_nuubos_libretro.so"
PGO_DIR="$ROOT/package/libretro/libretro-melonds/pgo"
HOST="root@192.168.5.36"
SECONDS_PER_GAME=90
GAMES=()

while [ "$#" -gt 0 ]; do
    case "$1" in
        -H) HOST="$2"; shift 2 ;;
        -t) SECONDS_PER_GAME="$2"; shift 2 ;;
        *) GAMES+=("$1"); shift ;;
    esac
done

if [ "${#GAMES[@]}" -eq 0 ]; then
    echo "[FAIL] no games given (GAME_ID:MODE)"
    exit 2
fi
if ! grep -qa '__gcov' "$CORE"; then
    echo "[FAIL] $CORE is not the instrumented core (step 1)"
    exit 1
fi

SSH=(ssh -o BatchMode=yes -o ConnectTimeout=5 "$HOST")
LIB=/usr/lib/libretro/melonds_nuubos_libretro.so

"${SSH[@]}" 'nuubos-emuctl status | grep -q "^state=idle"' || { echo "[FAIL] target unreachable or a game is running"; exit 1; }
scp -q "$CORE" "$HOST:/tmp/melonds-pgo-gen.so" || { echo "[FAIL] copy core"; exit 1; }
"${SSH[@]}" "U=\$(ls -d /userdata/users/*/ | head -1); rm -rf /tmp/nuubos-pgo /tmp/nuubos-pgo-states; cp -a \"\$U/states/nds\" /tmp/nuubos-pgo-states 2>/dev/null; mount --bind /tmp/melonds-pgo-gen.so $LIB" || { echo "[FAIL] mount"; exit 1; }

for G in "${GAMES[@]}"; do
    ID="${G%%:*}"
    MODE="${G#*:}"
    echo "training $ID $MODE (${SECONDS_PER_GAME} s)"
    "${SSH[@]}" "nuubos-emuctl launch $ID $MODE >/dev/null && sleep $SECONDS_PER_GAME; nuubos-emuctl quit >/dev/null; sleep 10; tail -2 /run/nuubos/emud.log | head -1"
done

"${SSH[@]}" "umount $LIB; U=\$(ls -d /userdata/users/*/ | head -1); [ -d /tmp/nuubos-pgo-states ] && rm -rf \"\$U/states/nds\" && cp -a /tmp/nuubos-pgo-states \"\$U/states/nds\"; rm -f /tmp/melonds-pgo-gen.so; ls /tmp/nuubos-pgo/*.gcda >/dev/null" || { echo "[FAIL] no profile written"; exit 1; }
rm -f "$PGO_DIR"/*.gcda
mkdir -p "$PGO_DIR"
"${SSH[@]}" 'cd /tmp/nuubos-pgo && tar -cf - *.gcda' | tar -xf - -C "$PGO_DIR" || { echo "[FAIL] fetch profile"; exit 1; }
"${SSH[@]}" 'rm -rf /tmp/nuubos-pgo /tmp/nuubos-pgo-states'
echo "[PASS] $(ls "$PGO_DIR"/*.gcda | wc -l) profile files in $PGO_DIR (step 3 next)"
