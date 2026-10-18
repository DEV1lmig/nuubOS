# opts.sh: sourced by the on-device dev scripts (copy it to /tmp with them).
# opt_set FILE KEY VALUE sets a core option and remembers the user's line;
# opt_restore FILE puts back only those keys. A whole-file copy back undid
# emud's one-time core option updates that ran during the test (2026-10-18).
# The scripts no longer copy save states back either: the states folder is
# synced by Syncthing and an older copy loses its conflicts.
OPT_SAVE=/tmp/opt-restore.$$
opt_set() {
	grep -q "^$2|" "$OPT_SAVE" 2>/dev/null || echo "$2|$(grep "^$2 = " "$1")" >> "$OPT_SAVE"
	sed -i "/^$2 = /d" "$1"
	echo "$2 = \"$3\"" >> "$1"
}
opt_restore() {
	[ -f "$OPT_SAVE" ] || return 0
	while IFS='|' read -r k l; do
		sed -i "/^$k = /d" "$1"
		[ -n "$l" ] && echo "$l" >> "$1"
	done < "$OPT_SAVE"
	rm -f "$OPT_SAVE"
}
