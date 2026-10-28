#!/bin/sh
# launchloop.sh ROUNDS WAIT GAME:MODE ... : launch each game, play WAIT seconds,
# kill -9 (no quit save), repeat; stops at the first kernel oops. Log /tmp/ll.log.
R=$1; W=$2; shift 2
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
o0=$(dmesg | grep -c -E "Oops|BUG|Poison|Redzone|kfence|Object padding|corrupt")
r=1
while [ $r -le $R ]; do
	for x in "$@"; do
		G=${x%%:*}; M=${x#*:}
		nuubos-emuctl launch $G $M >/dev/null
		sleep $W
		p=$(dmesg | grep -c "Purging"); f=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)
		P=$(pidof -s retroarch); fps=$(nuubos-emuctl status | grep '^fps=' | cut -d= -f2)
		echo "$(cut -d' ' -f1 /proc/uptime) round $r $G pid=$P fps=$fps purges=$p avail=${f}M gpu=$(cat /sys/class/devfreq/1800000.gpu/max_freq) t=$(cat /sys/class/thermal/thermal_zone*/temp | tr "\n" " ")"
		[ -n "$P" ] && kill -9 $P
		for k in $(seq 1 20); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done
		if [ $(dmesg | grep -c -E "Oops|BUG|Poison|Redzone|kfence|Object padding|corrupt") -gt $o0 ]; then
			echo "OOPS after round $r $G"; dmesg > /userdata/ll-dmesg-$(date +%H%M%S).txt; exit 2
		fi
		sleep 2
	done
	r=$((r+1))
done
echo DONE
