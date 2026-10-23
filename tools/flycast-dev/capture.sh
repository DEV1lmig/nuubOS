#!/bin/sh
# capture.sh SECONDS: profile the RUNNING game session without touching it
# (the user plays and says when): per-thread busy %, emulated fps each
# second and pcsample of the two busiest threads for SECONDS. Writes
# /tmp/cap.maps and /tmp/cap.<tid>; never launches, pauses or kills.
T=${1:-8}
P=$(pidof retroarch.real || pidof retroarch) || { echo "no game running"; exit 1; }
cp /proc/$P/maps /tmp/cap.maps
tt() { for t in /proc/$P/task/*; do echo "${t##*/} $(cut -d' ' -f14,15 $t/stat)"; done; }
tt > /tmp/cap.t1; sleep 2; tt > /tmp/cap.t2
B=$(awk 'NR==FNR{a[$1]=$2+$3;next}{print $2+$3-a[$1], $1}' /tmp/cap.t1 /tmp/cap.t2 | sort -rn | head -4)
echo "$B" | awk '{printf "%s %d%%  ", $2, $1/2}'; echo
rm -f /tmp/cap.[0-9]*
for t in $(echo "$B" | head -2 | awk '{print $2}'); do /tmp/pcsample $t $T > /tmp/cap.$t & done
fps=""; i=0; while [ $i -lt $T ]; do sleep 1; fps="$fps $(nuubos-emuctl status | grep '^fps=' | cut -d= -f2)"; i=$((i+1)); done
wait
echo "fps:$fps"; ls /tmp/cap.[0-9]*
