#!/bin/sh
# dcbench.sh SECONDS SAMPLES GAME[:MODE] [KEY=VALUE ...]: Dreamcast speed on
# the device. Sets the Flycast core options given (opts.sh, put back at the
# end), launches GAME (MODE default new), waits SECONDS, then prints SAMPLES
# GET_FPS readings (retro_run calls a second = emulated vblanks), the busy %
# of RetroArch's threads, GPU busy and the CPU clock. Copy opts.sh and
# gpubusy.sh (tools/melonds-dev) to /tmp with it. Never while the user plays.
T=$1; N=$2; X=$3; shift 3
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
U=$(ls -d /userdata/users/*/ | head -1); C=$U/appdata/retroarch/retroarch-core-options.cfg
S=$U/appdata/nuubos-emulation/settings.conf
cp $S /tmp/dcb-settings.conf; . /tmp/opts.sh
nuubos-emuctl set overlay 1 >/dev/null; nuubos-emuctl set overlay_items "" >/dev/null
for kv in "$@"; do opt_set $C "${kv%%=*}" "${kv#*=}"; done
G=${X%%:*}; M=${X#*:}; [ "$M" = "$X" ] && M=new
nuubos-emuctl launch $G $M >/dev/null
sleep $T
P=$(pidof retroarch)
tt() { for t in /proc/$P/task/*; do echo "${t##*/}:$(cat $t/comm | tr ' ' _) $(cut -d' ' -f14,15 $t/stat | tr ' ' +)"; done; }
a=$(tt); out=""; fr=""
i=0; while [ $i -lt $N ]; do sleep 1; out="$out $(nuubos-emuctl status | grep '^fps=' | cut -d= -f2)"; fr="$fr $(($(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq)/1000))"; i=$((i+1)); done
b=$(tt)
echo "fps:$out"; echo "mhz:$fr"
echo "$a" > /tmp/dcb-a; echo "$b" > /tmp/dcb-b
awk -v n=$N 'NR==FNR{split($2,x,"+");a[$1]+=x[1]+x[2];next}{split($2,x,"+");b[$1]+=x[1]+x[2]} END{for(k in b){p=(b[k]-a[k])/n; if(p>=3) printf "%s %d%%  ",k,p}; print ""}' /tmp/dcb-a /tmp/dcb-b
g() { for f in /proc/$P/fdinfo/*; do awk -v f=${f##*/} '/drm-engine-fragment/{a=$2} /drm-engine-vertex-tiler/{b=$2} END{if (a) print f, a, b}' $f; done; }
echo 1 > /sys/class/drm/card1/device/profiling; g > /tmp/dcb-ga; sleep 3; g > /tmp/dcb-gb; echo 0 > /sys/class/drm/card1/device/profiling
awk 'NR==FNR{a[$1]=$2;v[$1]=$3;next}{d=$2-a[$1]; if(d>m){m=d;w=$3-v[$1]}} END{printf "gpu fragment %.0f%% vertex %.0f%%\n", m/3e7, w/3e7}' /tmp/dcb-ga /tmp/dcb-gb
[ -n "$SHOT" ] && /tmp/drm-grab /dev/dri/card0 $SHOT >/dev/null 2>&1
[ "$(pidof retroarch)" = "$P" ] && kill -9 $P
for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done
opt_restore $C; cp /tmp/dcb-settings.conf $S
