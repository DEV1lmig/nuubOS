#!/bin/sh
# glsurvey.sh WAIT SECONDS GAME:MODE ... : per game fps, CPU of retroarch/labwc/home,
# GPU busy of retroarch and labwc, GPU and CPU frequency residency, battery power,
# DRM planes. Kills its own session with -9 (no quit save).
W=$1; T=$2; shift 2
mount | grep -q debugfs || mount -t debugfs none /sys/kernel/debug
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
U=/userdata/users/$(cat /run/nuubos/user/active); S=$U/appdata/nuubos-emulation/settings.conf
cp $S /tmp/gls-settings.conf
nuubos-emuctl set overlay 1 >/dev/null; nuubos-emuctl set overlay_items "" >/dev/null
DF=/sys/class/devfreq/1800000.gpu; PROF=/sys/class/drm/card1/device/profiling
BAT=$(ls -d /sys/class/power_supply/*bat* 2>/dev/null | head -1)
cpu() { [ -n "$1" ] && [ -d /proc/$1 ] && awk '{print $14+$15}' /proc/$1/stat || echo 0; }
gpu() { for f in /proc/$1/fdinfo/*; do awk -v f=${f##*/} '/drm-engine-fragment/{a=$2} /drm-engine-vertex-tiler/{b=$2} END{if (a!="") print f, a, b}' $f 2>/dev/null; done; }
# busy % of the fd with the largest fragment delta
gdelta() { echo "$1" > /tmp/gls-ga; echo "$2" > /tmp/gls-gb; awk -v t=$T 'NR==FNR{a[$1]=$2;v[$1]=$3;next}{d=$2-a[$1]; if(d>=m){m=d;w=$3-v[$1]}} END{printf "frag %d%% vtx %d%%", m/(t*1e7), w/(t*1e7)}' /tmp/gls-ga /tmp/gls-gb; }
fst() { awk 'NR>2 && $1 ~ /^\*?[0-9]/ {gsub(/\*/,"",$1); sub(/:$/,"",$1); print $1, $NF}' $DF/trans_stat; }
cst() { cat /sys/devices/system/cpu/cpu0/cpufreq/stats/time_in_state 2>/dev/null; }
run() {
	label=$1; P=$(pidof -s retroarch); L=$(pidof -s labwc); H=$(pidof -s nuubui-home)
	echo 1 > $PROF
	ra=$(cpu $P); la=$(cpu $L); ha=$(cpu $H); ga=$(gpu $P); gla=$(gpu $L)
	fst > /tmp/gls-fa; cst > /tmp/gls-ca
	fps=""; pw=0; n=0; i=0
	while [ $i -lt $T ]; do
		sleep 1
		[ -n "$P" ] && fps="$fps $(nuubos-emuctl status | grep '^fps=' | cut -d= -f2)"
		if [ -n "$BAT" ]; then
			c=$(cat $BAT/current_now); v=$(cat $BAT/voltage_now)
			pw=$((pw + (c < 0 ? -c : c) / 1000 * (v / 1000) / 1000)); n=$((n+1))
		fi
		i=$((i+1))
	done
	rb=$(cpu $P); lb=$(cpu $L); hb=$(cpu $H); gb=$(gpu $P); glb=$(gpu $L)
	fst > /tmp/gls-fb; cst > /tmp/gls-cb
	echo 0 > $PROF
	echo "== $label"
	[ -n "$P" ] && echo "fps:$fps"
	echo "cpu% retroarch $(( (rb-ra)/T )) labwc $(( (lb-la)/T )) home $(( (hb-ha)/T ))   bat $(cat $BAT/status 2>/dev/null) ~$((pw / (n?n:1))) mW"
	[ -n "$P" ] && echo "gpu retroarch $(gdelta "$ga" "$gb")"
	echo "gpu labwc $(gdelta "$gla" "$glb")"
	echo "gpu MHz residency: $(awk 'NR==FNR{a[$1]=$2;next}{d=$2-a[$1]; t+=d; r[$1]=d} END{for(k in r) if (r[k]>0) printf "%d:%d%% ", k/1e6, 100*r[k]/t}' /tmp/gls-fa /tmp/gls-fb)"
	echo "cpu MHz residency: $(awk 'NR==FNR{a[$1]=$2;next}{d=$2-a[$1]; t+=d; r[$1]=d} END{for(k in r) if (100*r[k]/t>=3) printf "%d:%d%% ", k/1000, 100*r[k]/t}' /tmp/gls-ca /tmp/gls-cb)"
	echo "planes: $(awk '/^plane\[/{on=0} /crtc=crtc-/{on=1} on && /format=/{printf "%s ", $1} on && /^\t\tsize=/{printf "%s | ", $1}' /sys/kernel/debug/dri/0/state)"
}
[ "$1" = home ] && { shift; run "home idle"; }
for x in "$@"; do
	G=${x%%:*}; M=${x#*:}; [ "$M" = "$x" ] && M=slot:1
	nuubos-emuctl launch $G $M >/dev/null
	sleep $W
	run "$G $M ($(nuubos-emuctl status | grep -E '^(system|core)=' | cut -d= -f2 | tr '\n' ' '))"
	P=$(pidof retroarch); [ -n "$P" ] && kill -9 $P
	for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done; sleep 3
done
cp /tmp/gls-settings.conf $S
