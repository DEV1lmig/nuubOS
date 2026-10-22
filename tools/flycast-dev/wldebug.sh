#!/bin/sh
# wldebug.sh GAME[:MODE] WAIT [SECONDS]: RetroArch with WAYLAND_DEBUG=client
# for SECONDS (default 3) after WAIT seconds; prints the surface commits and
# frame callbacks with their timestamps (ms) to /tmp/wl.log. Test only.
X=$1; W=$2; T=${3:-3}
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
cp /usr/bin/retroarch /tmp/retroarch.real
cat > /tmp/retroarch.wrap <<'WRAP'
#!/bin/sh
WAYLAND_DEBUG=client exec /tmp/retroarch.real "$@" 2>/tmp/wl-all.log
WRAP
chmod +x /tmp/retroarch.wrap
mount --bind /tmp/retroarch.wrap /usr/bin/retroarch
G=${X%%:*}; M=${X#*:}; [ "$M" = "$X" ] && M=new
: > /tmp/wl-all.log
nuubos-emuctl launch $G $M >/dev/null; sleep 3; umount /usr/bin/retroarch
sleep $W; P=$(pidof retroarch.real)
: > /tmp/wl-all.log; sleep $T
grep -E "wl_surface#[0-9]+\.commit|wl_callback#[0-9]+\.done|wl_surface#[0-9]+\.frame|attach" /tmp/wl-all.log > /tmp/wl.log
[ -n "$P" ] && [ "$(pidof retroarch.real)" = "$P" ] && kill -9 $P
for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done
wc -l /tmp/wl.log
