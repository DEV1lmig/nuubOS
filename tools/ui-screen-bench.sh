#!/bin/sh
# On-target UI benchmark (dev tool, never shipped; copied to /tmp with
# /tmp/uinput-nav). One persistent virtual pad (uinput-nav "@"), so the
# "controller connected" toast and its composition do not pollute the runs.
# Usage: ui-screen-bench.sh "label|PATTERN|INTERVAL_MS" ...  (patterns as
# uinput-nav; mind that A on Home starts the focused game)
# Prints per sequence: frames (Slint refresh_lazy fps log, needs
# SLINT_DEBUG_PERFORMANCE=refresh_lazy,console in /root/.config/labwc/environment),
# Home UI-thread CPU and GPU fragment time per press and per frame.
P=$(pidof nuubui-home)
gpu() { cat /proc/$P/fdinfo/* 2>/dev/null | awk '$1=="drm-client-id:"{id=$2} $1=="drm-engine-fragment:"{v[id]=$2} END{for(i in v)s+=v[i]; print s+0}'; }
cpu() { awk '{print $14+$15}' /proc/$P/task/$P/stat; }
rm -f /tmp/nav.in /tmp/nav.out; mkfifo /tmp/nav.in
/tmp/uinput-nav @ < /tmp/nav.in > /tmp/nav.out 2>&1 &
exec 3> /tmp/nav.in
sleep 12
echo 1 > /sys/bus/platform/devices/1800000.gpu/profiling
for spec in "$@"; do
  label=${spec%%|*}; rest=${spec#*|}; pat=${rest%%|*}; iv=${rest#*|}
  n=$(printf '%s' "$pat" | tr -cd 'RLUDABNWSEM' | wc -c)
  N=$(wc -l < /run/nuubos/labwc.log); c0=$(cpu); g0=$(gpu); d0=$(grep -c done /tmp/nav.out)
  echo "$pat $iv" >&3
  while [ "$(grep -c done /tmp/nav.out)" -le "$d0" ]; do sleep 0.1; done; sleep 0.8
  c1=$(cpu); g1=$(gpu)
  fr=$(tail -n +$N /run/nuubos/labwc.log | grep "frames per" | awk '{s+=$5} END{print s+0}')
  fps=$(tail -n +$N /run/nuubos/labwc.log | grep "frames per" | awk '{printf "%s ", $5}')
  [ "$n" -gt 0 ] || n=1; [ "$fr" -gt 0 ] || fr=1
  echo "$label: presses=$n frames~$fr cpu/press=$(( (c1-c0)*10/n ))ms gpu/press=$(( (g1-g0)/1000000/n ))ms cpu/frame=$(( (c1-c0)*10000/fr ))us gpu/frame=$(( (g1-g0)/1000/fr ))us fps:[$fps] ctx=$(cat /run/nuubos/ui-context)"
done
echo 0 > /sys/bus/platform/devices/1800000.gpu/profiling
exec 3>&-; sleep 0.5; rm -f /tmp/nav.in
