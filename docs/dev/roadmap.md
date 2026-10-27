# Roadmap

## Release plan (user decision 2026-10-07)
Supersedes the 2026-10-05 sequence (Settings polish → OOB → Home → emulation → OSD/in-game QM), all since implemented.
1. Alpha = the features implemented today (Web Mode excluded).
2. Alpha → RC: bug fixing and polish only; no new Epics or platform work. Blockers found in the current step are never skipped. Exception (user decision 2026-10-12): OTA with A/B system slots, channels on GitHub Releases and automatic rollback (EPIC-048, system.md), done before the RC so the SD1 layout is fixed before there are users. Also pulled forward (user request 2026-10-14): native cheats, LAN netplay and Get More Emulators (EPIC-013, emulation.md).
3. RC 1 = 1.0.0-rc.1 (2026-10-20, versioning in docs/technical/releasing.md). Linux 7.3 was pulled forward into it on user request (released 2026-10-18, platform.md). After the RC, in order: deep sleep (suspend-to-RAM instead of s2idle; first stage pulled forward and implemented 2026-10-11 on user request, platform.md); wireless casting EPIC-029 (screen casting pulled forward and implemented 2026-10-13 on user request: Google Cast, MS-MICE, DLNA, streaming-media.md; still needs for RTL8821CS — rtw88 exposes no P2P modes — and a Cedrus H.264 encoder — mainline Cedrus is decode-only; until then mirroring = software encoding); Spotify Connect EPIC-056 (Connect receiver through Product Audio, librespot candidate, redistribution terms to verify); Web Mode EPIC-030 (implemented, parked).
EPIC-014 (standalone emulators) removed 2026-10-07: RetroArch/libretro is the only backend.

## Diagnostic mode
Pulled forward into rc.2 and implemented 2026-10-22 (user request): docs/dev/system.md "Diagnostic mode". Still open: pstore/ramoops so that crashes and freezes keep their kernel log (needs a reserved-memory region in the DT).

## Community requests after 1.0 (Reddit, RC3 feedback, 2026-10-26)
Backlog for after 1.0.0 Stable; not prioritized yet, user decision per item; four of them go into RC4 (open-items.md "RC4 plan").
- USB device mode: MTP (copy games to the console from a PC over the USB cable, the main ask) and ADB. Today: web panel, SMB, SFTP, USB drives. Fits EPIC-024 (USB & External Accessories); needs the USB OTG port in peripheral mode (configfs gadget, e.g. umtprd) per board, and a choice between charging/host and device mode.
- Power key: -> RC4 (user 2026-10-26): short press = Sleep directly, long press = the power menu (Restart, Power Off). Today a short press opens the power menu (Controls wiki page). An option in Settings → Power, EPIC-045.
- Theme customization beyond the Moods: own colours, and replacing "nuubOS" at the top left with the user's name in the same font. EPIC-008 (Themes) / EPIC-003 (per-user).
- Web browser: EPIC-030 Web Mode (implemented, parked).
- Terminal: -> RC4 (user 2026-10-26), done 2026-10-27: app installed by default (user), foot + root's login shell, on-screen keyboard (system.md "Terminal application").
- Hide RetroArch: -> RC4 (user 2026-10-26): a switch to hide the RetroArch entry of the Quick Menu, to prevent accidental changes (per user, maybe tied to child profiles). EPIC-005.
- Multi-disc (.m3u): -> RC4 (user 2026-10-26): choose and change the disc from the Quick Menu instead of RetroArch's menu (disk control interface: eject, index, insert). EPIC-005 / EPIC-019.
- More shaders and user bezels: a choice of shaders beyond the CRT look, own bezel images per system/game; check what happens on HDMI (other resolution and aspect). EPIC-018.
- x86 build (old Intel NUCs): out of scope until the H700 is finished (other SoCs decided only after that, user 2026-10).
- Per-system settings (EPIC-004, RC3 plan item 3, not delivered in RC3): after 1.0.0 (user 2026-10-26).
