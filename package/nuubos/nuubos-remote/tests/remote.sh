#!/bin/sh
# Host test: nuubos-remotectl credential/policy. The Web administration
# daemon has its own unit tests: cargo test in package/nuubos/nuubos-remote/src.
# Run: sh package/nuubos/nuubos-remote/tests/remote.sh
HERE="$(cd "$(dirname "$0")" && pwd)"
CTL="$HERE/../src/nuubos-remotectl"
T="$(mktemp -d)"
trap 'rm -rf "${T:?}"' EXIT
FAIL=0
ck() { if eval "$2"; then :; else echo "[FAIL] $1"; FAIL=1; fi; }

# Fake tools: libraryctl, biosctl.
mkdir -p "$T/bin"
cat > "$T/bin/libraryctl" <<'EOC'
#!/bin/sh
echo "$*" >> "$LOG"
case "$1" in
  status) printf 'user=u\nscanning=0\ngames=2\nsystem=snes\tSuper Nintendo\t2\t1400\t#000\t\nend=1\n' ;;
  games) printf 'game=0123456789abcdef\tsnes\tMario "World"\t\t1400\t0\t0\t1\t0\nend=1\n' ;;
  delete) [ "$2" = 0123456789abcdef ] && [ "$3" = confirm ] && echo OK ;;
  scan) echo OK ;;
esac
EOC
printf '#!/bin/sh\nprintf "system=psx\\tPlayStation\\tpcsx\\tmissing\\t0\\t1\\nfile=psx\\tscph5501.bin\\t1\\tmissing\\tUS BIOS\\nend=1\\n"\n' > "$T/bin/biosctl"
# Fake busybox mkpasswd: deterministic "hash" of the password on fd 0.
printf '#!/bin/sh\nprintf "\\$6\\$t\\$%%s\\n" "$(md5sum | cut -c1-16)"\n' > "$T/bin/mkpasswd"
chmod +x "$T/bin/"*
export PATH="$T/bin:$PATH" LOG="$T/calls"

R="$T/root"
mkdir -p "$R/etc" "$R/state/ssh/root"
printf 'root::::::::\ndaemon:*:::::::\n' > "$R/etc/shadow"
ctl() { NUUBOS_ROOT="$R" NUUBOS_REMOTE_NO_DAEMONS=1 sh "$CTL" "$@"; }

ctl start
PW="$(ctl credential show)"
ck "credential generated, formatted" 'echo "$PW" | grep -Eq "^[a-z2-9]{4}-[a-z2-9]{4}-[a-z2-9]{4}$"'
ck "credential root-only" '[ "$(stat -c %a "$R/state/remote/credential")" = 600 ]'
hash_of() { printf '%s' "$1" | mkpasswd -m sha512 -P 0; }
ck "root password = credential" '[ "$(head -n1 "$R/etc/shadow")" = "root:$(hash_of "$PW"):::::::" ] && [ "$(sed -n 2p "$R/etc/shadow")" = "daemon:*:::::::" ]'
ck "shadow root-only" '[ "$(stat -c %a "$R/etc/shadow")" = 600 ]'
printf 'root::::::::\ndaemon:*:::::::\n' > "$R/etc/shadow"
ck "start re-applies shadow after an image update" 'ctl start; [ "$(head -n1 "$R/etc/shadow")" = "root:$(hash_of "$PW"):::::::" ]'
ck "no web digest file any more" '[ ! -e "$R/state/remote/web.htdigest" ]'
ck "services disabled by default" 'ctl status | grep -q "^ssh=0" && ctl status | grep -q "^web=0" && ctl status | grep -q "^smb=0"'
ck "restart keeps the credential" 'ctl start; [ "$(ctl credential show)" = "$PW" ]'
ck "enable web" '[ "$(ctl set web 1)" = OK ] && ctl status | grep -q "^web=1"'
ck "bad service rejected" '! ctl set telnet 1 >/dev/null'
ctl credential regenerate >/dev/null
PW2="$(ctl credential show)"
ck "regenerate changes it" '[ "$PW2" != "$PW" ]'
ck "regenerate updates root password" '[ "$(head -n1 "$R/etc/shadow")" = "root:$(hash_of "$PW2"):::::::" ]'

# Keys no longer sign in: an authorized_keys file does not enable SSH.
R2="$T/root2"; mkdir -p "$R2/etc" "$R2/state/ssh/root"
printf 'root::::::::\n' > "$R2/etc/shadow"; echo "ssh-ed25519 AAAA dev" > "$R2/state/ssh/root/authorized_keys"
NUUBOS_ROOT="$R2" NUUBOS_REMOTE_NO_DAEMONS=1 sh "$CTL" start
ck "SSH off by default even with keys" 'NUUBOS_ROOT="$R2" NUUBOS_REMOTE_NO_DAEMONS=1 sh "$CTL" status | grep -q "^ssh=0"'
ck "two devices, two credentials" '[ "$(NUUBOS_ROOT="$R2" NUUBOS_REMOTE_NO_DAEMONS=1 sh "$CTL" credential show)" != "$PW2" ]'

[ "$FAIL" = 0 ] && echo "[PASS] remote"
exit "$FAIL"
