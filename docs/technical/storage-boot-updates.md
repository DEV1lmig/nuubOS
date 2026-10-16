# Storage, boot and updates

## SD1 layout

The image contains partitions 1 to 3. On the first boot, `S04nuubos-userdata` adds
the rest (`usr/lib/nuubos/partitions.sh`).

| # | Label | Type | Size | Content |
|---|---|---|---|---|
| 1 | — | ext4 | 2 GiB | System slot A (the flashed image) |
| 2 | `NUUBOS_BOOT` | vfat | 16 MiB | `nuubos.cfg` (device), `nuubos-slot.cfg` (A/B state) |
| 3 | `NUUBOS_STATE` | ext4 | 128 MiB | Configuration, per-user settings, secrets, SSH host keys |
| 4 | — | extended | — | Container for 5 and 6 |
| 5 | — | ext4 | 2 GiB | System slot B (empty until the first update) |
| 6 | `USERDATA` | exFAT | rest | ROMs, BIOS, saves, states, screenshots, media, library, profiles |

- SYSTEM and STATE are never shown to the user as storage.
- USERDATA is exFAT so that the card can be read on any PC.
- Reset System Settings and every recovery action keep ROMs, BIOS, saves and
  USERDATA.

### SD2, the games card

SD2 holds content only (ROMs and media), formatted exFAT, FAT32 or ext4. Removing
it never loses progress: saves stay on SD1, its games show as unavailable until it
comes back. A card nuubOS did not prepare is never formatted without an explicit,
confirmed request. The console backup goes to SD2 by default when the device has a
slot (or to SD1 or a USB drive).

## Boot

1. SPL and U-Boot (raw at 8 KiB on SD1) load TF-A.
2. U-Boot `PREBOOT` reads `DEVICE` from `nuubos.cfg` and maps it to a device tree.
   An empty or unknown value stops the boot with a message: a wrong device tree is
   never guessed.
3. U-Boot reads `nuubos-slot.cfg` to choose the system slot (below), then boots
   `extlinux.conf` from it with `root=` and `nuubos.slot=` set.
4. BusyBox init runs `/etc/init.d/S??*`: splash, STATE (`S03`), USERDATA and first
   boot partitioning (`S04`), storage, services, then the UI (`S50nuubos-ui`).

Device names (`DEVICE=`): `rg-sp`, `rg28xx`, `rg34xx`, `rg34xx-sp`, `rg34xx-sp-v2`,
`rg35xx-2024`, `rg35xx-2024-v6`, `rg35xx-h`, `rg35xx-h-v6`, `rg35xx-plus`,
`rg35xx-plus-v6`, `rg35xx-pro`, `rg35xx-sp`, `rg35xx-sp-v2`, `rg40xx-h`,
`rg40xx-h-v2`, `rg40xx-v`, `rg40xx-v-v2`, `rgcubexx`.

## A/B system slots

`nuubos-slot.cfg` holds three U-Boot variables:

- `nuubos_slot`: the good slot.
- `nuubos_try`: a slot on trial, or `none`.
- `nuubos_tries`: attempts left for the trial slot.

A trial boot spends one try and writes the file back. With no tries left, or when
the trial slot cannot be loaded, U-Boot boots the good slot (then slot A, then the
generic distro boot). A kernel panic restarts after 10 s (`panic=10`); a CPU lockup
panics; a hang of the whole SoC is reset by the hardware watchdog after 16 s. Each
of these spends a try. Once nuubUI is up on the new slot, Home confirms it: it
becomes the good slot.

If U-Boot gave up on the new slot, the next boot on the old slot restores the copy
of STATE taken before the update and the user sees "The previous version was
restored". U-Boot, SPL and TF-A are never updated over the air.

## OTA updates

`nuubos-updatectl` (libcurl, libsodium) and `nuubos-update-apply` implement EPIC-048.

### Channels

`nightly`, `rc` and `stable`. A device follows one channel, stored in
`/state/update/channel`. The default is the channel the image was published on
(`NUUBOS_UPDATE_DEFAULT_CHANNEL` in `nuubos-update.mk`). Changing the channel in
Settings → System → Software Update forgets any downloaded update.

### Publication

Each channel is a fixed GitHub release named `channel-<channel>` (a pre-release,
never "Latest") that only holds the current manifest and its signature:

```
https://github.com/baf93/nuubOS/releases/download/channel-<channel>/nuubos-update.manifest
https://github.com/baf93/nuubOS/releases/download/channel-<channel>/nuubos-update.manifest.sig
```

The payload lives in a versioned release `v<version>-<serial>` that the manifest
points to.

### Manifest (format 2)

Plain `key=value` lines:

| Key | Meaning |
|---|---|
| `format` | `2` |
| `channel` | Channel it was published for |
| `version` | Product version (`VERSION` file) |
| `serial` | Build serial: Unix time of the image build, orders releases |
| `build` | `git describe` of the build |
| `payload` | `<tag>/<file>`, relative to the releases download URL |
| `size`, `sha256` | Size and SHA-256 of the compressed payload |
| `image_size`, `image_sha256` | Size and SHA-256 of the system image after decompression |
| `notes` | Release notes, one line (`\n` = new line), at most 4 KB |

The signature is Ed25519 over the manifest file. The public key is compiled into
`nuubos-updatectl` from `board/nuubos/common/ota/update-key.pub`.

A device offers an update only if the signature verifies, the format is 2, the
channel matches its own and the serial is higher than the running one. An old
signed manifest can therefore never downgrade a device.

### Install

1. Download (resumable, free-space check) into `/userdata/.nuubos-update`, verify
   SHA-256.
2. Install (needs more than 30 % battery): a console backup first, then the payload
   is decompressed onto the inactive slot, checked against `image_sha256`, read
   back.
3. The pre-power hook marks the new slot for trial (3 tries) and saves STATE.
4. Restart. Home confirms the new slot once the UI is up.

The daily automatic check (on by default) only notifies. It never downloads or
installs on its own.

### Release key

There is no key file. The Ed25519 seed is derived from a passphrase with scrypt
(`tools/ota/release-key.py`) at every signature. The passphrase is kept by the
maintainer. Losing it means devices need a reflash to trust a new key. Releasing:
[releasing.md](releasing.md).
