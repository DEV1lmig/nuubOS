#!/bin/sh
# prof.sh GAME MODE WAIT SECONDS: perf samples (tid ip) + maps of a running game; kills it
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
echo -1 > /proc/sys/kernel/perf_event_paranoid
nuubos-emuctl launch $1 $2 >/dev/null; sleep $3
P=$(pidof retroarch); cat /proc/$P/maps > /tmp/prof.maps
for t in /proc/$P/task/*; do echo "${t##*/} $(cat $t/comm)"; done > /tmp/prof.threads
nuubos-emuctl status | grep ^fps
cd /tmp; ./perf record -F 1999 -p $P -o /tmp/perf.data -- sleep $4 >/dev/null 2>&1
nuubos-emuctl status | grep ^fps
./perf script -i /tmp/perf.data -F tid,ip > /tmp/prof.samples 2>/dev/null
kill -9 $P
