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
#        MODE = resume | new | slot:N (nuubos-emuctl launch), optionally
#        :LAYOUT (melonds_screen_layout, _ for spaces) after it
#   3. rebuild the normal core with the new profile:
#        ./scripts/compile.sh libretro-melonds-dirclean libretro-melonds
#      and deploy it (tools/deploy-target.sh /usr/lib/libretro/melonds_nuubos_libretro.so).
#
# The instrumented core is bind-mounted over the installed one only while
# the games run (nothing is installed), by tools/melonds-dev/pgo-train.sh on
# the device: Syncthing is stopped meanwhile, RetroArch ends with SIGTERM
# (a normal exit writes the counters but no quit state) and the user's NDS
# saves/states are put back byte- and mtime-identical before Syncthing
# restarts (a quit save plus an rm -rf/cp restore made Syncthing keep the
# test files and rename the user's originals to sync-conflicts).

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

"${SSH[@]}" 'nuubos-emuctl status | grep -q "^state=idle"' || { echo "[FAIL] target unreachable or a game is running"; exit 1; }
scp -q "$CORE" "$HOST:/tmp/melonds-pgo-gen.so" || { echo "[FAIL] copy core"; exit 1; }
scp -q "$ROOT/tools/melonds-dev/pgo-train.sh" "$ROOT/tools/melonds-dev/opts.sh" "$HOST:/tmp/" || { echo "[FAIL] copy scripts"; exit 1; }
COUNT=$("${SSH[@]}" "sh /tmp/pgo-train.sh $SECONDS_PER_GAME ${GAMES[*]}" | tee /dev/stderr | tail -1)
[ "${COUNT:-0}" -gt 0 ] 2>/dev/null || { echo "[FAIL] no profile written"; exit 1; }
rm -f "$PGO_DIR"/*.gcda
mkdir -p "$PGO_DIR"
"${SSH[@]}" 'cd /tmp/nuubos-pgo && tar -cf - *.gcda' | tar -xf - -C "$PGO_DIR" || { echo "[FAIL] fetch profile"; exit 1; }
"${SSH[@]}" 'rm -rf /tmp/nuubos-pgo /tmp/pgo-snap /tmp/melonds-pgo-gen.so'
echo "[PASS] $(ls "$PGO_DIR"/*.gcda | wc -l) profile files in $PGO_DIR (step 3 next)"
