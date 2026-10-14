#!/bin/sh
# Wi-Fi stability soak for the RTL8821CS Deep LPS qualification
# (docs/dev/connectivity.md, 2026-10-14). Dev tool, never part of the image.
#
# On the host:
#   tools/wifi-soak.sh install root@IP     copy to the target, start, survive reboots
#   tools/wifi-soak.sh status root@IP      print the log summary
#   tools/wifi-soak.sh uninstall root@IP   stop and remove (the log stays)
# On the target (internal):
#   wifi-soak.sh run                        one sample every 5 minutes
#
# Log: /userdata/wifi-soak.log, one line per sample; on a new firmware
# Deep Power warning the dmesg is saved next to it.

LOG=/userdata/wifi-soak.log
INTERVAL=300
TARGET_SCRIPT=/userdata/wifi-soak.sh
INIT=/etc/init.d/S99wifi-soak

sample()
{
	boot=$(cut -c1-8 /proc/sys/kernel/random/boot_id)
	up=$(cut -d' ' -f1 /proc/uptime | cut -d. -f1)
	warn=$(dmesg | grep -c "failed to ack driver for")
	leave=$(dmesg | grep -c "failed to leave lps")
	deep=$(cat /sys/module/rtw88_core/parameters/disable_lps_deep 2>/dev/null)
	ps=$(iw dev wlan0 get power_save 2>/dev/null | cut -d: -f2 | tr -d ' ')
	link=$(iw dev wlan0 link 2>/dev/null | head -1 | cut -c1-12)
	gw=$(ip route 2>/dev/null | awk '/^default/ {print $3; exit}')
	loss=-
	[ -n "$gw" ] && loss=$(ping -c 3 -W 2 "$gw" 2>/dev/null | sed -n 's/.* \([0-9]*\)% packet loss.*/\1/p')
	echo "$(date '+%F %T') boot=$boot up=${up}s disable_lps_deep=$deep ps=$ps link=${link:-none} gw_loss=${loss:--}% deep_power_warn=$warn lps_leave_fail=$leave" >> "$LOG"
	if [ "$warn" -gt 0 ] && [ ! -e "/userdata/wifi-soak-dmesg-$boot.txt" ]; then
		dmesg > "/userdata/wifi-soak-dmesg-$boot.txt"
	fi
}

case "${1:-}" in
run)
	while :; do
		sample
		sleep "$INTERVAL"
	done
	;;
install)
	[ -n "$2" ] || { echo "usage: $0 install root@IP" >&2; exit 2; }
	scp -q "$0" "$2:$TARGET_SCRIPT" &&
	ssh "$2" "printf '%s\n' '#!/bin/sh' '# Wi-Fi soak test (tools/wifi-soak.sh); remove with uninstall' 'case \"\$1\" in start) start-stop-daemon -S -b -m -p /run/wifi-soak.pid -x /bin/sh -- $TARGET_SCRIPT run ;; stop) start-stop-daemon -K -p /run/wifi-soak.pid ;; esac' > $INIT && chmod 755 $INIT && $INIT start && echo installed"
	;;
status)
	[ -n "$2" ] || { echo "usage: $0 status root@IP" >&2; exit 2; }
	ssh "$2" "n=\$(wc -l < $LOG); echo \"samples=\$n\"; awk '{for(i=1;i<=NF;i++) if(\$i ~ /^(boot|deep_power_warn|lps_leave_fail|gw_loss)=/) {split(\$i,a,\"=\"); if(a[1]==\"boot\") b=a[2]; else if (a[2]+0>0) bad[b\" \"a[1]]=a[2]}} END {for(k in bad) print \"PROBLEM \" k \"=\" bad[k]}' $LOG; tail -3 $LOG; ls /userdata/wifi-soak-dmesg-*.txt 2>/dev/null"
	;;
uninstall)
	[ -n "$2" ] || { echo "usage: $0 uninstall root@IP" >&2; exit 2; }
	ssh "$2" "$INIT stop; rm -f $INIT $TARGET_SCRIPT; echo removed"
	;;
*)
	echo "usage: $0 install|status|uninstall root@IP" >&2
	exit 2
	;;
esac
