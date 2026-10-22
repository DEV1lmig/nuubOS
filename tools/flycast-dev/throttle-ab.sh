#!/bin/sh
# throttle-ab.sh ROUNDS SECONDS: Mesa swap throttle settings compared on the
# Dreamcast slot 8 states, alternated, one line per run into /tmp/tab.txt:
# round game config fps frames>20ms frames>25ms %>20ms. Uses ftrun.sh.
R=${1:-3}; T=${2:-20}
: > /tmp/tab.txt
r=1
while [ $r -le $R ]; do
	for g in 27415d60aff88447:DoA2 52c1f96b96d08784:LeMans 994b11440a59be40:Shenmue2 583fd36fe06c8cd6:CrazyTaxi; do
		for c in "d1:NUUBOS_THROTTLE_DEPTH=1" "d2:NUUBOS_THROTTLE_DEPTH=2" "d2s8:NUUBOS_THROTTLE_DEPTH=2 NUUBOS_THROTTLE_SOFT_MS=8"; do
			l=$(RA_ENV="${c#*:}" sh /tmp/ftrun.sh $T 120 ${g%%:*}:slot:8 | tail -1)
			echo "$r ${g#*:} ${c%%:*} $(echo "$l" | awk '{print $4, $15, $17, $20}')" >> /tmp/tab.txt
		done
	done
	r=$((r + 1))
done
echo done >> /tmp/tab.txt
