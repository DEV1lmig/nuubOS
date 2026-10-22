#!/usr/bin/env bash
set -uo pipefail

AREA="H6"
source "$NUBOS_ROOT/tests/platform/lib/common.sh"

# ---------------------------------------------------------------------------
# Boot storage discovery
# ---------------------------------------------------------------------------

root_dev="$(
    remote "sed -n 's/.*root=\([^ ]*\).*/\1/p' /proc/cmdline" 2>/dev/null ||
    true
)"

# SD1 layout (usr/lib/nuubos/partitions.sh, A/B system slots): p1 slot A,
# p2 NUUBOS_BOOT, p3 STATE, p4 extended holding p5 slot B and p6 USERDATA.
case "$root_dev" in
    /dev/mmcblk*p1)
        disk="${root_dev%p1}"
        ;;
    /dev/mmcblk*p5)
        disk="${root_dev%p5}"
        ;;
    *)
        fail "unable to derive TF1 boot disk from root=${root_dev:-unknown}"
        exit 0
        ;;
esac

p1="${disk}p1"
p2="${disk}p2"
p3="${disk}p3"
p4="${disk}p4"
p5="${disk}p5"
p6="${disk}p6"

disk_name="${disk#/dev/}"
p1_name="${p1#/dev/}"
p2_name="${p2#/dev/}"
p3_name="${p3#/dev/}"
p4_name="${p4#/dev/}"
p5_name="${p5#/dev/}"
p6_name="${p6#/dev/}"

pass "TF1 boot disk derived from root: $disk (running slot ${root_dev#$disk})"

# Keep raw storage state with the regression artifacts.
remote "
echo '===== SFDISK ====='
sfdisk -d '$disk' 2>/dev/null || true

echo
echo '===== BLKID ====='
blkid '$p1' '$p2' '$p3' '$p5' '$p6' 2>/dev/null || true

echo
echo '===== MOUNTS ====='
cat /proc/mounts | grep -E '(/state|/userdata|${disk_name})' || true
" > "$NUBOS_RESULT_DIR/H6-storage-layout.txt" 2>&1 || true

# ---------------------------------------------------------------------------
# Required first-boot storage tools
# ---------------------------------------------------------------------------

storage_tools=(
    sfdisk
    partx
    blockdev
    blkid
    mkfs.exfat
)

missing_tools=()

for tool in "${storage_tools[@]}"; do
    remote_has "$tool" || missing_tools+=("$tool")
done

if ((${#missing_tools[@]} == 0)); then
    pass "first-boot storage tools present"
else
    fail "first-boot storage tools missing: ${missing_tools[*]}"
fi

# ---------------------------------------------------------------------------
# TF1 partition presence
# ---------------------------------------------------------------------------

remote "[ -b '$p1' ]" \
    && pass "rootfs partition present ($p1)" \
    || fail "rootfs partition missing ($p1)"

remote "[ -b '$p2' ]" \
    && pass "NUUBOS_BOOT partition present ($p2)" \
    || fail "NUUBOS_BOOT partition missing ($p2)"

remote "[ -b '$p3' ]" \
    && pass "NUUBOS_STATE partition present ($p3)" \
    || fail "NUUBOS_STATE partition missing ($p3)"

remote "[ -b '$p4' ]" \
    && pass "extended partition present ($p4)" \
    || fail "extended partition missing ($p4)"

remote "[ -b '$p5' ]" \
    && pass "system slot B partition present ($p5)" \
    || fail "system slot B partition missing ($p5)"

remote "[ -b '$p6' ]" \
    && pass "NUUBOS_DATA partition present ($p6)" \
    || fail "NUUBOS_DATA partition missing ($p6)"

if ! remote "[ -b '$p1' ] && [ -b '$p2' ] && [ -b '$p3' ] && [ -b '$p5' ] && [ -b '$p6' ]"; then
    skip "TF1 geometry/filesystem checks unavailable"
    exit 0
fi

# ---------------------------------------------------------------------------
# TF1 geometry
#
# rootfs       = slot A, 2 GiB, starts at sector 2048, bootable
# NUUBOS_BOOT  = 16 MiB, immediately follows rootfs
# NUUBOS_STATE = 128 MiB, immediately follows NUUBOS_BOOT
# extended     = immediately follows NUUBOS_STATE and consumes the rest of TF1
# slot B       = 2 GiB, first logical partition (1 MiB EBR gap)
# NUUBOS_DATA  = second logical partition, the rest of the extended partition
# ---------------------------------------------------------------------------

p1_start="$(remote "cat '/sys/class/block/$p1_name/start'" || true)"
p1_size="$(remote "cat '/sys/class/block/$p1_name/size'" || true)"
p2_start="$(remote "cat '/sys/class/block/$p2_name/start'" || true)"
p2_size="$(remote "cat '/sys/class/block/$p2_name/size'" || true)"
p3_start="$(remote "cat '/sys/class/block/$p3_name/start'" || true)"
p3_size="$(remote "cat '/sys/class/block/$p3_name/size'" || true)"
p4_start="$(remote "cat '/sys/class/block/$p4_name/start'" || true)"
p5_start="$(remote "cat '/sys/class/block/$p5_name/start'" || true)"
p5_size="$(remote "cat '/sys/class/block/$p5_name/size'" || true)"
p6_start="$(remote "cat '/sys/class/block/$p6_name/start'" || true)"
p6_size="$(remote "cat '/sys/class/block/$p6_name/size'" || true)"
disk_sectors="$(remote "blockdev --getsz '$disk'" || true)"

if [[ "$p1_start" == "2048" && "$p1_size" == "4194304" ]]; then
    pass "rootfs geometry 2 GiB at sector 2048"
else
    fail "rootfs geometry start=${p1_start:-?} size=${p1_size:-?}"
fi

if remote "sfdisk -d '$disk' 2>/dev/null | grep -F '$p1 :' | grep -qw bootable"; then
    pass "rootfs partition is bootable"
else
    fail "rootfs partition is not bootable"
fi

if remote "sfdisk -d '$disk' 2>/dev/null | grep -F '$p2 :' | grep -Eq 'type=(c|0[cC]|0x0[cC])([[:space:]]|$)'"; then
    pass "NUUBOS_BOOT MBR type 0x0c"
else
    fail "NUUBOS_BOOT MBR type is not 0x0c"
fi

expected_p2_start=$(( ${p1_start:-0} + ${p1_size:-0} ))

if [[ "$p2_start" == "$expected_p2_start" &&
      "$p2_size" == "32768" ]]; then
    pass "NUUBOS_BOOT geometry 16 MiB immediately after rootfs"
else
    fail "NUUBOS_BOOT geometry start=${p2_start:-?} size=${p2_size:-?}"
fi

expected_p3_start=$(( ${p2_start:-0} + ${p2_size:-0} ))

if [[ "$p3_start" == "$expected_p3_start" &&
      "$p3_size" == "262144" ]]; then
    pass "NUUBOS_STATE geometry 128 MiB immediately after NUUBOS_BOOT"
else
    fail "NUUBOS_STATE geometry start=${p3_start:-?} size=${p3_size:-?}"
fi

if remote "sfdisk -d '$disk' 2>/dev/null | grep -F '$p3 :' | grep -Eq 'type=(83|0x83)([[:space:]]|$)'"; then
    pass "NUUBOS_STATE MBR type 0x83"
else
    fail "NUUBOS_STATE MBR type is not 0x83"
fi

expected_p4_start=$(( ${p3_start:-0} + ${p3_size:-0} ))

if [[ "$p4_start" == "$expected_p4_start" &&
      $(( ${p4_start:-1} % 2048 )) -eq 0 ]]; then
    pass "extended partition starts after NUUBOS_STATE with 1 MiB alignment"
else
    fail "extended partition start=${p4_start:-?} expected=$expected_p4_start"
fi

# The sysfs size of an extended partition is the 1-2 sector link, not its
# extent: read the table size from sfdisk.
p4_table_size="$(remote "sfdisk -d '$disk' 2>/dev/null | sed -n 's|^$p4 :.*size= *\([0-9]*\).*|\1|p'" || true)"
expected_p4_size=$(( ${disk_sectors:-0} - ${p4_start:-0} ))

if [[ "$p4_table_size" == "$expected_p4_size" && "$expected_p4_size" -gt 0 ]]; then
    pass "extended partition consumes remaining TF1 capacity"
else
    fail "extended partition size=${p4_table_size:-?} expected=$expected_p4_size"
fi

if remote "sfdisk -d '$disk' 2>/dev/null | grep -F '$p4 :' | grep -Eq 'type=(5|f)([[:space:]]|,|$)'"; then
    pass "extended partition MBR type 0x05"
else
    fail "extended partition MBR type is not 0x05"
fi

if [[ "$p5_start" == "$(( ${p4_start:-0} + 2048 ))" && "$p5_size" == "4194304" ]]; then
    pass "system slot B geometry 2 GiB at the start of the extended partition"
else
    fail "system slot B geometry start=${p5_start:-?} size=${p5_size:-?}"
fi

expected_p6_start=$(( ${p5_start:-0} + ${p5_size:-0} + 2048 ))
expected_p6_size=$(( ${disk_sectors:-0} - expected_p6_start ))

if [[ "$p6_start" == "$expected_p6_start" ]]; then
    pass "NUUBOS_DATA starts after slot B with 1 MiB alignment"
else
    fail "NUUBOS_DATA start=${p6_start:-?} expected=$expected_p6_start"
fi

if [[ "$p6_size" == "$expected_p6_size" && "$expected_p6_size" -gt 0 ]]; then
    pass "NUUBOS_DATA consumes remaining TF1 capacity"
else
    fail "NUUBOS_DATA size=${p6_size:-?} expected=$expected_p6_size"
fi

if remote "
sfdisk -d '$disk' 2>/dev/null |
grep -F '$p6 :' |
grep -Eq 'type=7[[:space:]]*$'
"; then
    pass "USERDATA MBR type 0x07"
else
    fail "USERDATA MBR type is not 0x07"
fi

# ---------------------------------------------------------------------------
# rootfs / BOOT / STATE filesystems
# ---------------------------------------------------------------------------

p1_type="$(remote "blkid -s TYPE -o value '$p1' 2>/dev/null" || true)"
p1_label="$(remote "blkid -s LABEL -o value '$p1' 2>/dev/null" || true)"

[[ "$p1_type" == "ext4" ]] \
    && pass "rootfs filesystem ext4" \
    || fail "rootfs filesystem ${p1_type:-unknown}"

[[ "$p1_label" == "rootfs" ]] \
    && pass "rootfs label rootfs" \
    || fail "rootfs label ${p1_label:-unknown}"

p2_type="$(remote "blkid -s TYPE -o value '$p2' 2>/dev/null" || true)"
p2_label="$(remote "blkid -s LABEL -o value '$p2' 2>/dev/null" || true)"

[[ "$p2_type" == "vfat" ]] \
    && pass "NUUBOS_BOOT filesystem FAT" \
    || fail "NUUBOS_BOOT filesystem ${p2_type:-unknown}"

[[ "$p2_label" == "NUUBOS_BOOT" ]] \
    && pass "NUUBOS_BOOT label NUUBOS_BOOT" \
    || fail "NUUBOS_BOOT label ${p2_label:-unknown}"

p3_type="$(remote "blkid -s TYPE -o value '$p3' 2>/dev/null" || true)"
p3_label="$(remote "blkid -s LABEL -o value '$p3' 2>/dev/null" || true)"

[[ "$p3_type" == "ext4" ]] \
    && pass "STATE filesystem ext4" \
    || fail "STATE filesystem ${p3_type:-unknown}"

[[ "$p3_label" == "NUUBOS_STATE" ]] \
    && pass "STATE label NUUBOS_STATE" \
    || fail "STATE label ${p3_label:-unknown}"

if remote "
awk -v d='$p3' '
\$1 == d && \$2 == \"/state\" && \$3 == \"ext4\" {
    rw = (\$4 ~ /(^|,)rw(,|$)/)
    noatime = (\$4 ~ /(^|,)noatime(,|$)/)
    if (rw && noatime)
        ok=1
}
END { exit !ok }
' /proc/mounts
"; then
    pass "STATE mounted rw,noatime on /state"
else
    fail "STATE mount policy unexpected"
fi

# ---------------------------------------------------------------------------
# Persistent STATE contract
# ---------------------------------------------------------------------------

remote "grep -qx 'NUUBOS_CONFIG_VERSION=6' /state/config/nuubos.conf" \
    && pass "STATE config version 6" \
    || fail "STATE config version invalid"

# ---------------------------------------------------------------------------
# Persistent user-profile foundation
# ---------------------------------------------------------------------------

remote "grep -qx 'NUUBOS_USERS_VERSION=1' /state/users/.nuubos-users" \
    && pass "user registry version 1" \
    || fail "user registry missing or invalid"

login_mode="$(
    remote "sed -n 's/^USER_LOGIN_MODE=//p' /state/config/nuubos.conf | head -n 1" \
        || true
)"

case "$login_mode" in
    AUTO|DEFAULT|SELECT)
        pass "user login mode valid ($login_mode)"
        ;;
    *)
        fail "user login mode invalid (${login_mode:-missing})"
        ;;
esac

default_user="$(
    remote "sed -n 's/^DEFAULT_USER=//p' /state/config/nuubos.conf | head -n 1" \
        || true
)"

if [[ "$default_user" =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]]; then
    pass "default user UUID valid ($default_user)"
else
    fail "default user UUID invalid (${default_user:-missing})"
fi

if [[ -n "$default_user" ]] &&
   remote "[ -d '/state/users/$default_user' ]"; then
    pass "default user profile directory present"
else
    fail "default user profile directory missing"
fi

if [[ -n "$default_user" ]] &&
   remote "grep -qx 'NUUBOS_USER_PROFILE_VERSION=1' '/state/users/$default_user/profile.conf'"; then
    pass "default user profile version 1"
else
    fail "default user profile version invalid"
fi

if [[ -n "$default_user" ]] &&
   remote "grep -qx 'USER_ID=$default_user' '/state/users/$default_user/profile.conf'"; then
    pass "default user profile identity consistent"
else
    fail "default user profile identity inconsistent"
fi

if [[ -n "$default_user" ]] &&
   remote "grep -qx 'NUUBOS_USER_SETTINGS_VERSION=1' '/state/users/$default_user/settings.conf'"; then
    pass "default user settings version 1"
else
    fail "default user settings missing or invalid"
fi

if [[ -n "$default_user" ]] &&
   remote "find '/state/users/$default_user/secrets' \
           -maxdepth 0 -type d -perm 700 -print 2>/dev/null |
           grep -qx '/state/users/$default_user/secrets'"; then
    pass "default user secrets directory protected"
else
    fail "default user secrets directory missing or permissions invalid"
fi

user_count="$(
    remote '
        n=0
        for d in /state/users/*; do
            [ -d "$d" ] || continue
            n=$((n + 1))
        done
        echo "$n"
    ' || true
)"
user_count="${user_count:-0}"

((user_count > 0)) \
    && pass "persistent user profiles present ($user_count)" \
    || fail "no persistent user profiles"

active_user="$(
    remote 'cat /run/nuubos/user/active 2>/dev/null' || true
)"

if ((user_count == 1)); then
    [[ "$active_user" == "$default_user" ]] \
        && pass "single-user active profile equals default user" \
        || fail "single-user active profile inconsistent"
elif [[ "$login_mode" == "DEFAULT" ]]; then
    # The default user is the one started at boot; Switch User can make
    # another profile active afterwards, and nothing records the boot
    # choice, so a different active profile is valid as long as it exists.
    if [[ "$active_user" == "$default_user" ]]; then
        pass "default-login active profile equals default user"
    elif [[ -n "$active_user" ]] &&
         remote "[ -f '/state/users/$active_user/settings.conf' ]"; then
        pass "default-login active profile is an existing profile"
        info "active profile differs from the default user (switched after boot)"
    else
        fail "default-login active profile inconsistent"
    fi
elif [[ -z "$active_user" ]]; then
    pass "multi-user selection has no premature active profile"
else
    # Observational suite: the user may already have picked a profile.
    remote "[ -f '/state/users/$active_user/profile.conf' ]" \
        && pass "multi-user active profile is a selected, existing user" \
        || fail "multi-user active profile invalid (${active_user})"
fi

storage_mode="$(
    remote "sed -n 's/^STORAGE_MODE=//p' /state/config/nuubos.conf" ||
    true
)"

case "$storage_mode" in
    AUTO|SINGLE|DUAL)
        pass "STATE storage mode valid ($storage_mode)"
        ;;
    *)
        fail "STATE storage mode invalid (${storage_mode:-missing})"
        ;;
esac

if remote "[ \"\$(readlink /etc/dropbear)\" = '/state/ssh/dropbear' ]"; then
    pass "Dropbear server state linked to STATE"
else
    fail "Dropbear server state not linked to STATE"
fi

if remote "[ \"\$(readlink /root/.ssh)\" = '/state/ssh/root' ]"; then
    pass "root SSH state linked to STATE"
else
    fail "root SSH state not linked to STATE"
fi

if remote "
find /state/ssh/dropbear \
    -maxdepth 1 \
    -type f \
    -name 'dropbear_*_host_key' |
grep -q .
"; then
    pass "persistent SSH server identity present"
else
    fail "persistent SSH server identity missing"
fi

remote "[ -f /state/ssh/root/id_dropbear ]" \
    && pass "persistent SSH client identity present" \
    || fail "persistent SSH client identity missing"

remote "[ -x /etc/init.d/S03nuubos-state ]" \
    && pass "S03 STATE initializer installed" \
    || fail "S03 STATE initializer missing"

# ---------------------------------------------------------------------------
# USERDATA filesystem
# ---------------------------------------------------------------------------

p6_type="$(remote "blkid -s TYPE -o value '$p6' 2>/dev/null" || true)"
p6_label="$(remote "blkid -s LABEL -o value '$p6' 2>/dev/null" || true)"

[[ "$p6_type" == "exfat" ]] \
    && pass "USERDATA filesystem exFAT" \
    || fail "USERDATA filesystem ${p6_type:-unknown}"

[[ "$p6_label" == "NUUBOS_DATA" ]] \
    && pass "USERDATA label NUUBOS_DATA" \
    || fail "USERDATA label ${p6_label:-unknown}"

if remote "
awk -v d='$p6' '
\$1 == d && \$2 == \"/userdata\" && \$3 == \"exfat\" {
    rw = (\$4 ~ /(^|,)rw(,|$)/)
    noatime = (\$4 ~ /(^|,)noatime(,|$)/)
    if (rw && noatime)
        ok=1
}
END { exit !ok }
' /proc/mounts
"; then
    pass "USERDATA mounted rw,noatime on /userdata"
else
    fail "USERDATA mount policy unexpected"
fi

remote "grep -qx 'NUUBOS_USERDATA_VERSION=2' /userdata/.nuubos-userdata" \
    && pass "USERDATA marker version 2" \
    || fail "USERDATA marker invalid"

remote "[ -x /etc/init.d/S04nuubos-userdata ]" \
    && pass "S04 USERDATA initializer installed" \
    || fail "S04 USERDATA initializer missing"

# ---------------------------------------------------------------------------
# USERDATA directory contract
# ---------------------------------------------------------------------------

userdata_dirs=(
applications
bios
cache
cheats
downloads
library
logs
music
ports
roms
themes
users
"users/$default_user"
"users/$default_user/saves"
"users/$default_user/states"
"users/$default_user/screenshots"
"users/$default_user/recordings"
"users/$default_user/library"
"users/$default_user/appdata"
)

legacy_user_dirs=(
saves
states
screenshots
recordings
)

legacy_present=()

for dir in "${legacy_user_dirs[@]}"; do
    remote "[ ! -e '/userdata/$dir' ]" ||
        legacy_present+=("$dir")
done

if ((${#legacy_present[@]} == 0)); then
    pass "legacy flat per-user USERDATA directories absent"
else
    fail "legacy flat USERDATA directories present: ${legacy_present[*]}"
fi

if [[ -n "$active_user" ]]; then
    if remote "
        awk '
            \$2 == \"/run/nuubos/userdata\" {
                found=1
            }

            END {
                exit !found
            }
        ' /proc/mounts
    "; then
        pass "active-user USERDATA runtime view mounted"
    else
        fail "active-user USERDATA runtime view missing"
    fi

    if remote "
        awk -v expected='/users/$active_user' '
            \$4 == expected &&
            \$5 == \"/run/nuubos/userdata\" {
                ok=1
            }

            END {
                exit !ok
            }
        ' /proc/self/mountinfo
    "; then
        pass "active-user USERDATA runtime view targets active profile"
    else
        fail "active-user USERDATA runtime view targets wrong profile"
    fi
fi

missing_dirs=()

for dir in "${userdata_dirs[@]}"; do
    remote "[ -d '/userdata/$dir' ]" || missing_dirs+=("$dir")
done

if ((${#missing_dirs[@]} == 0)); then
    pass "USERDATA directory skeleton complete"
else
    fail "USERDATA directories missing: ${missing_dirs[*]}"
fi

# Write/remove coverage is restricted to the explicitly invasive modes.
if [[ "$NUBOS_TEST_MODE" == "full" || "$NUBOS_TEST_MODE" == "stress" ]] && remote '
f="/userdata/.nuubos-regression-write.$$"
trap "rm -f \"$f\"" EXIT HUP INT TERM

printf "%s\n" "nuubOS-H6-write-test" > "$f" &&
grep -qx "nuubOS-H6-write-test" "$f" &&
rm -f "$f" &&
sync
'; then
    pass "USERDATA read/write/remove"
else
    if [[ "$NUBOS_TEST_MODE" != "full" && "$NUBOS_TEST_MODE" != "stress" ]]; then
        skip "USERDATA read/write/remove (full/stress only)"
    else
        fail "USERDATA read/write/remove"
    fi
fi

if remote '[ ! -e /state/migrations/userdata-init.pending ]'; then
    pass "USERDATA initialization transaction complete"
else
    fail "USERDATA initialization transaction still pending"
fi

# ---------------------------------------------------------------------------
# TF2 baseline
#
# TF2 policy (AUTO/SINGLE/DUAL) will be validated separately. The normal H6
# regression must never format or modify an inserted TF2 card.
# ---------------------------------------------------------------------------

if remote '[ -d /sys/class/mmc_host/mmc2 ]'; then
    pass "TF2 MMC host present"
else
    fail "TF2 MMC host missing"
fi

if remote '[ -b /dev/mmcblk2 ]'; then
    pass "TF2 card detected"
else
    skip "TF2 card not inserted"
fi

# ---------------------------------------------------------------------------
# SD2 games card and SD1 storage contract (nuubos-storagectl, platform.md
# "Storage"): S05 writes storage.status, `storagectl sd2 attach` sd2.status.
# ---------------------------------------------------------------------------

storage_status=/run/nuubos/storage/storage.status
sd2_status=/run/nuubos/storage/sd2.status

status_value() {
    remote "sed -n 's/^$2=//p' '$1' | head -n 1" || true
}

if remote "[ -f '$storage_status' ]"; then
    pass "SD1 storage status published"

    tf1_device="$(status_value "$storage_status" TF1_DEVICE)"
    storage_ready="$(status_value "$storage_status" STORAGE_READY)"
    effective_userdata="$(
        remote 'awk '"'"'$2=="/userdata"{s=$1} END{print s}'"'"' /proc/mounts' ||
        true
    )"

    [[ "$tf1_device" == "$p6" && "$effective_userdata" == "$p6" ]] \
        && pass "USERDATA is SD1 $p6 in status and mounts" \
        || fail "USERDATA device status=${tf1_device:-missing} mounted=${effective_userdata:-none} expected=$p6"

    case "$storage_ready" in
        1)
            remote "[ -e '/run/nuubos/storage/ready' ]" \
                && pass "storage ready file consistent" \
                || fail "storage ready file missing"
            ;;
        0)
            remote "[ ! -e '/run/nuubos/storage/ready' ]" \
                && pass "storage not-ready state consistent" \
                || fail "storage ready file unexpectedly present"
            ;;
        *)
            fail "invalid STORAGE_READY '${storage_ready:-missing}'"
            ;;
    esac
else
    fail "SD1 storage status missing"
fi

if remote "[ -f '$sd2_status' ]"; then
    pass "SD2 storage status published"

    sd2_slot="$(status_value "$sd2_status" SD2_SLOT)"
    sd2_state="$(status_value "$sd2_status" SD2_STATE)"
    sd2_path="$(status_value "$sd2_status" SD2_PATH)"

    case "$sd2_state" in
        none|empty|checking|ready|readonly|unreadable|ejected|ignored)
            pass "SD2 state valid ($sd2_state, slot=${sd2_slot:-?})"
            ;;
        *)
            fail "unknown SD2 state '${sd2_state:-missing}'"
            ;;
    esac

    if [[ "$sd2_state" == "ready" || "$sd2_state" == "readonly" ]]; then
        remote "awk -v p='$sd2_path' '\$2 == p { f=1 } END { exit !f }' /proc/mounts" \
            && pass "SD2 mounted at $sd2_path" \
            || fail "SD2 $sd2_state but not mounted at ${sd2_path:-?}"
    else
        remote 'awk '"'"'$1 ~ /^\/dev\/mmcblk2/ { f=1 } END { exit !f }'"'"' /proc/mounts' \
            && fail "SD2 mounted while state is $sd2_state" \
            || pass "SD2 not mounted while state is $sd2_state"
    fi

    # The games card must never replace SD1 USERDATA.
    [[ "$(remote 'awk '"'"'$2=="/userdata"{s=$1} END{print s}'"'"' /proc/mounts' || true)" != /dev/mmcblk2* ]] \
        && pass "SD2 never mounted as USERDATA" \
        || fail "SD2 mounted as USERDATA"
else
    fail "SD2 storage status missing"
fi

remote "[ -x /etc/init.d/S05nuubos-storage ]" \
    && pass "S05 storage selector installed" \
    || fail "S05 storage selector missing"

# ---------------------------------------------------------------------------
# Invasive storage qualification
#
# These helpers preserve and restore state, but they intentionally exercise
# configuration, service restarts, repair and recovery paths. Keep them out of
# quick so that quick remains strictly observational/read-only.
# ---------------------------------------------------------------------------

# lib/h6-storage-{backup,health,recovery}.sh qualified the retired DUAL
# TF1/TF2 USERDATA model (`storagectl storage-mode`, tf2.status, OWNED/FOREIGN
# cards); SD2 is a games card since 2026-10-11 and the console backup replaced
# the per-profile one (platform.md "Storage"). Not run until rewritten.
skip "storage backup/health/recovery qualification (retired DUAL storage model)"

# ---------------------------------------------------------------------------
# USB baseline
# ---------------------------------------------------------------------------

if remote '[ -d /sys/bus/usb/devices ]'; then
    pass "USB bus sysfs present"
else
    fail "USB bus sysfs missing"
fi

if [ "${USB_REQUIRE_OTG:-0}" = "1" ]; then
    if remote 'test "$(tr -d "\000" </proc/device-tree/soc/usb@5100000/dr_mode 2>/dev/null)" = "otg"'; then
        pass "USB0 device-tree mode OTG"
    else
        fail "USB0 device-tree mode is not OTG"
    fi

    if remote '[ -e /proc/device-tree/soc/phy@5100400/usb0_id_det-gpios ]'; then
        pass "USB0 ID detection described by device tree"
    else
        fail "USB0 ID detection missing from device tree"
    fi

    if remote '[ -e /proc/device-tree/soc/phy@5100400/usb0_vbus_power-supply ]'; then
        pass "USB0 external VBUS detection supply described"
    else
        fail "USB0 external VBUS detection supply missing"
    fi

    if remote '[ -e /proc/device-tree/soc/phy@5100400/usb0_vbus-supply ]'; then
        pass "USB0 switched VBUS supply described"
    else
        fail "USB0 switched VBUS supply missing"
    fi

    if remote 'readlink -f /sys/bus/platform/devices/5101000.usb/driver 2>/dev/null | grep -q "/ehci-platform$"'; then
        pass "USB0 EHCI host controller bound"
    else
        fail "USB0 EHCI host controller not bound"
    fi

    if remote 'readlink -f /sys/bus/platform/devices/5101400.usb/driver 2>/dev/null | grep -q "/ohci-platform$"'; then
        pass "USB0 OHCI host controller bound"
    else
        fail "USB0 OHCI host controller not bound"
    fi

    if remote 'grep -q "usb0_id_det" /sys/kernel/debug/gpio 2>/dev/null'; then
        pass "USB0 ID detection GPIO claimed"
    else
        fail "USB0 ID detection GPIO not claimed"
    fi

    if remote 'grep -q "regulator-usb0-vbus" /sys/kernel/debug/gpio 2>/dev/null'; then
        pass "USB0 switched VBUS GPIO claimed"
    else
        fail "USB0 switched VBUS GPIO not claimed"
    fi
fi
