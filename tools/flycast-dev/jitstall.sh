#!/bin/sh
# jitstall.sh GAME:MODE EVENT: samples EVENT (perf raw event, e.g. rE7) on
# the emulation thread and saves the sampled host code: /tmp/js.samples
# (tid ip) and /tmp/js.code (the SH4 dynarec code cache read from
# /proc/PID/mem, base address on its first line). Kills the session.
X=$1; E=$2; G=${X%%:*}; M=${X#*:}
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
nuubos-emuctl launch $G $M >/dev/null; sleep 12
P=$(pidof retroarch)
T=$(for t in /proc/$P/task/*; do echo "$(cut -d' ' -f14 $t/stat) ${t##*/}"; done | sort -n | tail -1 | cut -d' ' -f2)
cd /tmp; ./perf record -e $E -c 5000 -t $T -o /tmp/js.data -- sleep 4 >/dev/null 2>&1
./perf script -i /tmp/js.data -F tid,ip > /tmp/js.samples 2>/dev/null; rm -f /tmp/js.data
touch /tmp/fc-dumpblocks; sleep 2
L=$(sort /tmp/fc-blocks.txt | head -1 | cut -d' ' -f1); H=$(sort /tmp/fc-blocks.txt | tail -1)
E2=$(printf '%x' $((0x${H%% *} + 0x$(echo $H | cut -d' ' -f2))))
echo "$L $E2" > /tmp/js.range
dd if=/proc/$P/mem of=/tmp/js.code bs=4096 skip=$((0x$L / 4096)) count=$(( (0x$E2 - 0x$L) / 4096 + 2 )) 2>/dev/null
echo $(( (0x$L / 4096) * 4096 )) > /tmp/js.base
kill -9 $P
