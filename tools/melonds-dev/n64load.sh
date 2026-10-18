#!/bin/sh
# n64load.sh GAME...: fps, GPU busy and the busiest threads (5 s) of each game
for G in "$@"; do
  nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
  nuubos-emuctl launch $G new >/dev/null; sleep 30
  P=$(pidof retroarch)
  for t in /proc/$P/task/*; do echo "${t##*/} $(cat $t/comm) $(cut -d' ' -f14,15 $t/stat)"; done > /tmp/l1
  f1=$(nuubos-emuctl status | grep ^fps= | cut -d= -f2)
  sh /tmp/gpubusy.sh 5 > /tmp/gb
  for t in /proc/$P/task/*; do echo "${t##*/} $(cat $t/comm) $(cut -d' ' -f14,15 $t/stat)"; done > /tmp/l2
  echo "== $G fps $f1 $(cat /tmp/gb) hdmi $(cat /sys/class/drm/card*-HDMI-A-1/enabled 2>/dev/null | head -1)"
  paste /tmp/l1 /tmp/l2 | awk '{d=($7+$8)-($3+$4); if (d>15) printf "  %s %s %d%%\n", $1, $2, d*100/500}'
  kill -9 $P; for k in $(seq 1 20); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done; sleep 2
done
