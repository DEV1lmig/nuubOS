#!/bin/sh
#
# SD1 layout (EPIC-048 A/B system slots):
#   p1 system slot A (ext4, the flashed image)
#   p2 NUUBOS_BOOT (vfat: nuubos.cfg, nuubos-slot.cfg read by U-Boot)
#   p3 STATE
#   p4 extended partition, created on the first boot (S04) with
#   p5 system slot B (ext4, written by OTA updates)
#   p6 USERDATA (exFAT, rest of the card)
# The running slot is the root= U-Boot passed (p1 or p5).

NUUBOS_PART_BOOT=2
NUUBOS_PART_STATE=3
NUUBOS_PART_EXTENDED=4
NUUBOS_PART_SLOT_B=5
NUUBOS_PART_USERDATA=6
NUUBOS_SLOT_SECTORS=4194304

nuubos_boot_disk()
{
    NUUBOS_ROOT_DEV="$(sed -n 's/.*root=\([^ ]*\).*/\1/p' /proc/cmdline 2>/dev/null)"

    case "$NUUBOS_ROOT_DEV" in
        /dev/mmcblk*p1) printf '%s\n' "${NUUBOS_ROOT_DEV%p1}" ;;
        /dev/mmcblk*p5) printf '%s\n' "${NUUBOS_ROOT_DEV%p5}" ;;
        *) return 1 ;;
    esac
}

nuubos_partition()
{
    NUUBOS_DISK="$(nuubos_boot_disk)" || return 1
    printf '%sp%s\n' "$NUUBOS_DISK" "$1"
}
