#!/bin/sh
# fps-survey.sh SECONDS GAME[:MODE] ...: real-play fps (emud GET_FPS through the
# performance overlay) of each game, 10 samples after SECONDS; the user's NDS
# settings.conf and the core options it set are put back afterwards (opts.sh).
T=$1; shift
for w in 1 2 3 4 5; do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 2; done
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
U=$(ls -d /userdata/users/*/ | head -1); C=$U/appdata/retroarch/retroarch-core-options.cfg
S=$U/appdata/nuubos-emulation/settings.conf
cp $S /tmp/fs-settings.conf; . /tmp/opts.sh
OV=$(grep "^overlay=" $S | cut -d= -f2); OI=$(grep "^overlay_items=" $S | cut -d= -f2)
nuubos-emuctl set overlay 1 >/dev/null
for kv in $OPTS; do opt_set $C ${kv%%=*} "$(echo "${kv#*=}" | tr _ ' ')"; done
for x in "$@"; do
  G=${x%%:*}; M=${x#*:}; [ "$M" = "$x" ] && M=resume
  nuubos-emuctl launch $G $M >/dev/null
  sleep $T
  P=$(pidof retroarch); out=""
  i=0; while [ $i -lt 10 ]; do sleep 1; out="$out $(nuubos-emuctl status | grep '^fps=' | cut -d= -f2)"; i=$((i+1)); done
  echo "$G $M:$out"
  [ "$(pidof retroarch)" = "$P" ] && nuubos-emuctl quit >/dev/null
  for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done; sleep 2
done
opt_restore $C; cp /tmp/fs-settings.conf $S
