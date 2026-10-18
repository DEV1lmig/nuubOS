#!/bin/sh
# pgo-train.sh SECONDS GAME:MODE[:LAYOUT] ...: on the device, train the instrumented melonDS core
# (/tmp/melonds-pgo-gen.so, copied by tools/melonds-pgo.sh); MODE resume|new|slot:N, LAYOUT a
# melonds_screen_layout value with _ for spaces. Counters go to /tmp/nuubos-pgo.
# Syncthing stopped meanwhile; RetroArch ends with SIGTERM (normal exit writes the gcda, no quit state);
# the user's NDS saves/states are snapshotted and put back byte- and mtime-identical before Syncthing restarts.
T=$1; shift
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
U=$(ls -d /userdata/users/*/ | head -1); C=${U}appdata/retroarch/retroarch-core-options.cfg
. /tmp/opts.sh
/etc/init.d/S49nuubos-syncd stop >/dev/null 2>&1; sleep 2
pidof syncthing >/dev/null && { echo "syncthing still running"; /etc/init.d/S49nuubos-syncd start; exit 1; }
rm -rf /tmp/pgo-snap /tmp/nuubos-pgo; mkdir -p /tmp/pgo-snap
cp -a "${U}saves/nds" /tmp/pgo-snap/saves; cp -a "${U}states/nds" /tmp/pgo-snap/states
mount --bind /tmp/melonds-pgo-gen.so /usr/lib/libretro/melonds_nuubos_libretro.so
for x in "$@"; do
  G=$(echo $x | cut -d: -f1); M=$(echo $x | cut -d: -f2-3); L=$(echo $x | cut -d: -f4)
  case $M in slot:*) ;; *) M=$(echo $x | cut -d: -f2);; esac
  O=${U}appdata/nuubos-emulation/games/$G.conf; rm -f /tmp/pgo-game.conf
  if [ -n "$L" ]; then
    opt_set $C melonds_screen_layout "$(echo $L | tr _ ' ')"
    # the game's own remembered layout would win over the option
    [ -f $O ] && cp -a $O /tmp/pgo-game.conf && sed -i '/^nds_layout=/d' $O
  fi
  nuubos-emuctl launch $G $M >/dev/null; sleep $T
  P=$(pidof retroarch); kill -TERM $P; for i in $(seq 1 30); do [ -d /proc/$P ] || break; sleep 1; done
  echo "$x: $(tail -2 /run/nuubos/emud.log | head -1)"
  opt_restore $C
  [ -f /tmp/pgo-game.conf ] && cp -a /tmp/pgo-game.conf $O
  sleep 2
done
umount /usr/lib/libretro/melonds_nuubos_libretro.so
for d in saves states; do
  cd "${U}$d/nds"; for f in *; do [ -f "$f" ] && ! [ -e "/tmp/pgo-snap/$d/$f" ] && { echo "new $d/$f removed"; rm -f "$f"; }; done
  cd /tmp/pgo-snap/$d; for f in *; do [ -f "$f" ] || continue; cmp -s "$f" "${U}$d/nds/$f" && ! [ "$f" -nt "${U}$d/nds/$f" ] && ! [ "$f" -ot "${U}$d/nds/$f" ] || { echo "restored $d/$f"; cp -a "$f" "${U}$d/nds/$f"; }; done
done
/etc/init.d/S49nuubos-syncd start >/dev/null 2>&1
ls /tmp/nuubos-pgo/*.gcda 2>/dev/null | wc -l
