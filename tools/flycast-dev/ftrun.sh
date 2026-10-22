#!/bin/sh
# ftrun.sh SECONDS SKIP GAME[:MODE] [KEY=VALUE ...]: frame-time stability of
# one game on the device. RetroArch runs with /tmp/frametime.so preloaded
# (a wrapper bind-mounted over /usr/bin/retroarch for this launch only), the
# core options given are set (opts.sh, put back at the end), GAME is launched
# (MODE default new, e.g. slot:8), recorded for SECONDS, then this session
# alone is killed and frametime-report.sh prints the summary without the
# first SKIP frames. Copy frametime.so, frametime-report.sh and opts.sh
# (tools/melonds-dev) to /tmp. Never while the user plays.
T=$1; K=$2; X=$3; shift 3
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
U=$(ls -d /userdata/users/*/ | head -1); C=$U/appdata/retroarch/retroarch-core-options.cfg
. /tmp/opts.sh
for kv in "$@"; do opt_set $C "${kv%%=*}" "${kv#*=}"; done
cp /usr/bin/retroarch /tmp/retroarch.real
# RA_EXTRA="key = \"value\";..." adds RetroArch config keys for this launch
# (appended after emud's session.cfg); RA_ENV="VAR=value ..." overrides
# environment variables emud sets (e.g. NUUBOS_THROTTLE_DEPTH=1).
rm -f /tmp/ft-extra.cfg /tmp/ft-env
[ -n "$RA_EXTRA" ] && echo "$RA_EXTRA" | tr ';' '\n' > /tmp/ft-extra.cfg
[ -n "$RA_ENV" ] && echo "export $RA_ENV" > /tmp/ft-env
cat > /tmp/retroarch.wrap <<'WRAP'
#!/bin/sh
for a in "$@"; do
	if [ -f /tmp/ft-extra.cfg ] && [ "${a##*/}" = session.cfg ]; then
		a="$a|/tmp/ft-extra.cfg"
	fi
	set -- "$@" "$a"
	shift
done
[ -f /tmp/ft-env ] && . /tmp/ft-env
FRAMETIME_OUT=/tmp/frametime.bin LD_PRELOAD=/tmp/frametime.so exec /tmp/retroarch.real "$@"
WRAP
chmod +x /tmp/retroarch.wrap
rm -f /tmp/frametime.bin
mount --bind /tmp/retroarch.wrap /usr/bin/retroarch
G=${X%%:*}; M=${X#*:}; [ "$M" = "$X" ] && M=new
nuubos-emuctl launch $G $M >/dev/null
sleep 3
umount /usr/bin/retroarch
P=$(pidof retroarch.real || pidof retroarch)
fps=""; i=0; while [ $i -lt $T ]; do sleep 1; fps="$fps $(nuubos-emuctl status | grep '^fps=' | cut -d= -f2)"; i=$((i+1)); done
echo "fps:$fps"
[ -n "$P" ] && [ "$(pidof retroarch.real || pidof retroarch)" = "$P" ] && kill -9 $P
for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done
opt_restore $C
sh /tmp/frametime-report.sh /tmp/frametime.bin $K
