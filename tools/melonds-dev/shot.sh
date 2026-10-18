#!/bin/sh
# shot.sh VARIANT GAME SLOT SECONDS: run a test core, report fps, leave a grab marker
V=$1; G=$2; SL=$3; T=${4:-15}
for w in 1 2 3 4 5; do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 2; done
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
U=$(ls -d /userdata/users/*/ | head -1); C=$U/appdata/retroarch/retroarch-core-options.cfg
. /tmp/opts.sh
opt_set $C melonds_accuracy fast
for kv in $OPTS; do opt_set $C ${kv%%=*} "$(echo "${kv#*=}" | tr _ ' ')"; done
[ "$V" != stock ] && mount --bind /tmp/melonds-$V.so /usr/lib/libretro/melonds_nuubos_libretro.so
case $SL in new|resume) LM=$SL;; *) LM=slot:$SL;; esac
nuubos-emuctl launch $G $LM >/dev/null; sleep $T
P=$(pidof retroarch); grep -i -E "nera|nds" /proc/$P/maps; cat /proc/$P/task/*/comm | tr "\n" " "; echo; echo "pid $P threads $(ls /proc/$P/task | wc -l) $(grep -E "VmRSS|RssAnon|RssFile" /proc/$P/status | tr -s " \t" " " | tr "\n" " ")"; free -m | sed -n 2p
f=""; for i in 1 2 3 4 5 6; do sleep 1; f="$f $(nuubos-emuctl status | grep ^fps= | cut -d= -f2)"; done; echo "fps:$f"
touch /tmp/shot-ready; i=0; while [ -e /tmp/shot-ready ] && [ $i -lt 60 ]; do sleep 1; i=$((i+1)); done
[ "$(pidof retroarch)" = "$P" ] && nuubos-emuctl quit >/dev/null
for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done; sleep 2
[ "$V" != stock ] && umount /usr/lib/libretro/melonds_nuubos_libretro.so
opt_restore $C
tail -3 /run/nuubos/emud.log
