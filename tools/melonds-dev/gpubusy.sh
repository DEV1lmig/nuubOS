#!/bin/sh
# gpubusy.sh [SECONDS]: panfrost fragment / vertex-tiler busy % of retroarch
T=${1:-5}; P=$(pidof retroarch)
echo 1 > /sys/class/drm/card1/device/profiling
s() { for f in /proc/$P/fdinfo/*; do awk '/drm-engine-fragment/{f=$2} /drm-engine-vertex-tiler/{v=$2} END {if (f) print f, v}' $f; done | sort -n | tail -1; }
a=$(s); sleep $T; b=$(s)
echo 0 > /sys/class/drm/card1/device/profiling
echo "$a $b" | awk -v t=$T '{printf "fragment %.0f%% vertex %.0f%%\n", ($3-$1)/(t*1e7), ($4-$2)/(t*1e7)}'
