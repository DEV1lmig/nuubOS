# Architecture

nuubOS is a Buildroot-based Linux system for Allwinner H700 handhelds. A set of
small services owns the product behaviour; thin user interfaces show their state and
send them commands.

```
 ┌──────────────────────────────────────────────────────────────────────┐
 │  nuubUI (Home, Settings, apps)   Quick Menu + notifications + OSD    │  Rust + Slint
 │  Web administration (browser)    RetroArch / Moonlight / mpv / ...   │
 ├───────────────── labwc (wlroots Wayland compositor) ─────────────────┤
 │  Product services: one capability each, typed contract, no polling   │  C (mostly), Rust
 │  usersd libraryd emud audiod wifid bluetoothd controllersd inputd    │
 │  systemd(nuubOS) statusd notifyd jobd mediad streamd castd syncd ... │
 ├──────────────────────────────────────────────────────────────────────┤
 │  Standard Linux: PipeWire/WirePlumber, BlueZ, wpa_supplicant,         │
 │  D-Bus, udev, BusyBox init, Mesa Panfrost, V4L2 (Cedrus), ksmbd       │
 ├──────────────────────────────────────────────────────────────────────┤
 │  Linux 7.3 mainline + H700 patches · U-Boot (A/B slots) · TF-A        │
 └──────────────────────────────────────────────────────────────────────┘
```

## Principles

- **Services are the source of truth.** Each capability (users, library, audio,
  Wi-Fi, ...) has exactly one owning service. It exposes a contract made of
  commands, a state snapshot and typed change events. Clients take the snapshot,
  subscribe to changes, and send commands. They never reimplement policy.
- **No polling.** Interfaces react to events. An idle service sleeps in `poll()`
  with no timeout; timers are armed only while work is pending. Idle wakeups are
  measured on the device (`voluntary_ctxt_switches` over 30 s).
- **One path per behaviour.** Settings, the Quick Menu, the setup wizard, Home and
  the web administration all use the same services. For example, Sleep, Restart and
  Power Off go through one lifecycle path in `nuubos-systemd`, whoever asks.
- **Controller first.** Everything works with buttons. The UI uses logical
  actions (`menu_confirm`, `menu_back`, `face_north`, ...) and never brand labels.
- **Responsive layouts**: native layouts for 640x480 (4:3), 720x720 (square),
  720x480 and 16:9 TVs up to 1080p.
- **Everything visible is localized** (29 languages, index-based catalogs).

The development rules behind these are in [docs/dev/principles.md](../dev/principles.md).

## Contracts

Most services speak a line protocol on a Unix socket under `/run/nuubos/`:

- `STATUS`: one snapshot, `key=value` lines, ending with `end=1`.
- `SUBSCRIBE`: the same snapshot now and again after every change.
- Commands: one line, tab-separated arguments, reply `OK ...` or `ERR <code>`.
  Error codes are stable identifiers; the UI localizes them.

Some services (audio, Wi-Fi, Bluetooth, controllers, lighting, rumble) use D-Bus
on the system bus (`org.nuubOS.*`). Every service ships a CLI client
(`nuubos-<name>ctl`) that is used by tests, scripts and support.

## Services

| Service | Init script | Owns |
|---|---|---|
| `nuubos-systemd` | `S49nuubos-system` | Lifecycle (sleep, restart, power off), performance profile, idle sleep, charge limit, storage status, Reset System Settings |
| `nuubos-usersd` | `S46nuubos-users` | Users, avatars, login mode, setup-complete flag |
| `nuubos-localizationd` | `S48nuubos-localization` | Per-user language and keyboard layout |
| `nuubos-regionald` | `S47nuubos-regional` | Time zone, date and time, Wi-Fi regulatory country |
| `nuubos-statusd` | `S48nuubos-status` | Battery, clock, time sync, the daily update check |
| `nuubos-notifyd` | `S40nuubos-notifyd` | Notification router (typed events, Live Notifications) |
| `nuubos-jobd` | `S39nuubos-jobd` | Background jobs with progress and cancel |
| `nuubos-inputd` | `S47nuubos-inputd` | Input routing, game gamepads (uinput), hotkeys |
| `nuubos-controllersd` | `S48nuubos-controllersd` | Controller mapping, players, tester |
| `nuubos-rumbled`, `nuubos-lightingd`, `nuubos-rgbd` | `S48`/`S49`/`S07` | Rumble, RGB effects and signals, RGB hardware |
| `nuubos-audiod` | `S43nuubos-audiod` | Audio policy over PipeWire: outputs, volumes, system sounds |
| `nuubos-wifid`, `nuubos-bluetoothd` | `S44`/`S45` | Wi-Fi (wpa_supplicant), Bluetooth (BlueZ) |
| `nuubos-displayd` | `S08nuubos-displayd` | Brightness, screen off |
| `nuubos-libraryd` | `S47nuubos-library` | Game catalog, scans, history, collections, tags, search |
| `nuubos-emud` | `S49nuubos-emud` | Game sessions (RetroArch), save states, cheats, netplay |
| `nuubos-mediad` | `S49nuubos-mediad` | Music and video playback (mpv) |
| `nuubos-streamd` | `S49nuubos-streamd` | Moonlight and Steam Link sessions |
| `nuubos-castd` | `S49nuubos-castd` | Screen casting (Google Cast, Miracast over Infrastructure, DLNA) |
| `nuubos-syncd` | `S49nuubos-syncd` | Syncthing for the active user |
| `nuubos-admind` | started by `nuubos-remotectl` | Web administration API (behind lighttpd, HTTPS) |

Owned by tools rather than daemons: storage and the games card
(`nuubos-storagectl`), backup (`nuubos-sd2-backup`), files (`nuubos-filesctl`),
network shares (`nuubos-sharesctl`), recovery (`nuubos-recoveryctl`,
`nuubos-diag`), updates (`nuubos-updatectl`, `nuubos-update-apply`).

## User interface

- **nuubUI** (`package/nuubos/nuubui-home`): Home, Game Library, Settings, the
  setup wizard (OOB), Files, Media, Moonlight, Steam Link and Netplay pages.
  Rust with [Slint](https://slint.dev), rendered with GLES on Panfrost.
- **Quick Menu** (`package/nuubos/nuubos-quick-menu`): a separate process with
  Wayland layer surfaces over everything (Home, games, streams). It also draws
  notifications, the volume OSD, the power menu and the in-game closing screen.
- **labwc** (wlroots) is the compositor. Games and video are scanned out directly
  when possible; nuubOS patches wlroots/labwc only for platform needs (720-line
  client buffers on HDMI, panel rotation), never for product logic.
- **Web administration** (`package/nuubos/nuubos-remote`): a browser version of
  Settings plus library and file management, served over HTTPS by lighttpd with
  `nuubos-admind` (Rust) as the API.

## Storage

| Partition | Content |
|---|---|
| SD1 p1 | System slot A (ext4, 2 GiB) |
| SD1 p2 | `NUUBOS_BOOT` (vfat): `nuubos.cfg` (device), `nuubos-slot.cfg` (A/B state) |
| SD1 p3 | `STATE` (ext4, 128 MiB): configuration, profiles' settings, secrets |
| SD1 p5 | System slot B (2 GiB, created on first boot) |
| SD1 p6 | `USERDATA` (exFAT, rest of the card): ROMs, BIOS, saves, media, library |
| SD2 | Optional games card: ROMs and media only |

The system slots are read-only images replaced as a whole by updates. Nothing
device- or user-specific is stored in them. Details:
[storage-boot-updates.md](storage-boot-updates.md).

## Emulation

RetroArch is the only emulation backend. `nuubos-emud` launches it with a
per-user configuration plus a generated session configuration, talks to it over
stdin/stdout (patched commands with replies), and exposes common actions natively
(save/load state, slots, screenshots, cheats, netplay). "RetroArch" in the Quick
Menu opens RetroArch's own menu for advanced users. Cores and their licenses:
[package/libretro/README.md](../../package/libretro/README.md).

## Where things live in the repository

| Path | Content |
|---|---|
| `package/nuubos/` | nuubOS packages: services, UI, data |
| `package/libretro/`, `package/streaming/`, `package/thirdparty/` | RetroArch and cores, Moonlight and Steam Link runtime, other third-party packages |
| `board/nuubos/` | Rootfs overlays, init scripts, post-build/post-image scripts, kernel and U-Boot configuration |
| `patches/` | Patches to Linux, U-Boot, TF-A, Buildroot and packages |
| `configs/` | Buildroot defconfig |
| `scripts/` | Build entry point and build-time helpers |
| `tools/` | Developer tools: UI screenshots (`ui-shot`), deploy, OTA release |
| `docs/dev/` | Detailed engineering notes per area (the development log) |
| `docs/product/` | Product epics |
