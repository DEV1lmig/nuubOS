#!/bin/sh
# statetest.sh GAME[:MODE]: save state round trip in a session of its own.
# RetroArch runs with --verbose into /tmp/ra-state.log (wrapper bind-mounted
# over /usr/bin/retroarch for this launch only); saves slot 7, waits, grabs,
# loads it, grabs again. With slot rotation (default) the save goes to
# slot 8 when 7 already holds a state: only on the dev profile. Needs /tmp/drm-grab. Never while the user plays.
X=$1
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
cp /usr/bin/retroarch /tmp/retroarch.real
printf '#!/bin/sh\nexec /tmp/retroarch.real --verbose "$@" >/tmp/ra-state.log 2>&1\n' > /tmp/retroarch.wrap
chmod +x /tmp/retroarch.wrap
mount --bind /tmp/retroarch.wrap /usr/bin/retroarch
G=${X%%:*}; M=${X#*:}; [ "$M" = "$X" ] && M=new
nuubos-emuctl launch $G $M >/dev/null; sleep 3; umount /usr/bin/retroarch; sleep 5
P=$(pidof retroarch.real)
nuubos-emuctl slot 7 >/dev/null; sleep 1
nuubos-emuctl status | grep -q "^slot=7$" || { echo "slot is not 7: not saving"; kill -9 $P; exit 1; }
nuubos-emuctl save; sleep 1
/tmp/drm-grab /dev/dri/card0 /tmp/st-saved >/dev/null 2>&1
sleep 6; /tmp/drm-grab /dev/dri/card0 /tmp/st-later >/dev/null 2>&1
nuubos-emuctl load; sleep 2
/tmp/drm-grab /dev/dri/card0 /tmp/st-loaded >/dev/null 2>&1
[ -n "$P" ] && [ "$(pidof retroarch.real)" = "$P" ] && kill -9 $P
for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done
grep -i -E "serializ|state|lua|error|fail" /tmp/ra-state.log | grep -v -i "Config\]\|Environ" | tail -25
ls /tmp/st-*
