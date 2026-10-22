#!/bin/sh
# frametime-report.sh FILE [SKIP]: summary of a frametime.so recording (u32
# microseconds per presented frame). SKIP = frames to drop at the start
# (loading). Prints frames, mean fps, p50/p90/p99/max in ms and how many
# intervals took more than 20 ms (a vblank missed at 60 Hz), 25 ms and 34 ms
# (two or more missed): the stutter count, which a mean fps hides.
F=$1; S=${2:-0}
od -An -v -tu4 -w4 "$F" | awk -v s="$S" 'NR>s{print $1}' | sort -n | awk '
{ v[NR]=$1; t+=$1; if ($1>20000) a++; if ($1>25000) b++; if ($1>34000) c++ }
END {
	if (NR == 0) { print "no frames"; exit }
	printf "frames %d  fps %.1f  p50 %.1f  p90 %.1f  p99 %.1f  max %.1f ms  >20ms %d  >25ms %d  >34ms %d  (%.2f%% over 20)\n",
		NR, NR*1e6/t, v[int(NR*.5)+1]/1e3, v[int(NR*.9)+1]/1e3, v[int(NR*.99)+1]/1e3, v[NR]/1e3, a, b, c, 100*a/NR
}'
