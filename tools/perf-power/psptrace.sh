#!/bin/sh
# psptrace.sh N: launch PSP N times, log emud reply + RetroArch lifetime
for r in $(seq 1 $1); do
	rep=$(nuubos-emuctl launch b1454f1344673a6e slot:9 2>&1 | tr '\n' ' ')
	t0=$(cut -d' ' -f1 /proc/uptime); P=""; last=""
	for k in $(seq 1 40); do
		p=$(pidof -s retroarch); [ -n "$p" ] && P=$p && last=$(cut -d' ' -f1 /proc/uptime)
		[ -z "$p" ] && [ -n "$P" ] && break
		sleep 0.25
	done
	st=$(nuubos-emuctl status | tr '\n' ' ' | cut -c1-80)
	echo "run $r reply=[$rep] pid=$P alive_until=$last start=$t0 now=$(cut -d' ' -f1 /proc/uptime) $st"
	p=$(pidof -s retroarch); [ -n "$p" ] && kill -9 $p
	for k in $(seq 1 20); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done
	sleep 2
done
