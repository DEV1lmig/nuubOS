#!/bin/sh
# cpuab.sh GAME:MODE SECONDS: main-thread busy % and emulated fps of one launch
X=$1; T=$2
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
G=${X%%:*}; M=${X#*:}
nuubos-emuctl launch $G $M >/dev/null; sleep 6
P=$(pidof retroarch)
a=$(cut -d' ' -f14,15 /proc/$P/task/$P/stat); sleep $T; b=$(cut -d' ' -f14,15 /proc/$P/task/$P/stat)
f=$(nuubos-emuctl status | grep '^fps=' | cut -d= -f2)
[ "$(pidof retroarch)" = "$P" ] && kill -9 $P
for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done
echo "$a $b" | awk -v t=$T -v f=$f '{printf "main %.1f%%  fps %s\n", ($3+$4-$1-$2)/t, f}'
