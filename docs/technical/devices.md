# Devices

nuubOS ships one image for the Anbernic H700 family. The device is chosen
explicitly with `DEVICE=` in `nuubos.cfg` on `NUUBOS_BOOT`; U-Boot maps it to a
device tree (patch `patches/u-boot/platform/h700/0001-...device-selector.patch`).
There is no automatic detection, and an empty or unknown value stops the boot.

| `DEVICE` | Device tree | Screen | Sticks | Status |
|---|---|---|---|---|
| `rg40xx-v` | `sun50i-h700-anbernic-rg40xx-v.dtb` | 640x480 | 1 | Qualified |
| `rg40xx-v-v2` | `...rg40xx-v-v2-panel.dtb` | 640x480 | 1 | Qualified |
| `rgcubexx` | `...rgcubexx.dtb` | 720x720 | 2 | Qualified |
| `rg35xx-pro` | `...rg35xx-pro.dtb` | 640x480 | 2 | Qualified (new PWM backlight untested) |
| `rg35xx-2024`, `rg35xx-2024-v6` | `...rg35xx-2024(-rev6-panel).dtb` | 640x480 | 0 | Untested |
| `rg35xx-plus`, `rg35xx-plus-v6` | `...rg35xx-plus(-rev6-panel).dtb` | 640x480 | 0 | Untested |
| `rg35xx-h`, `rg35xx-h-v6` | `...rg35xx-h(-rev6-panel).dtb` | 640x480 | 2 | Untested |
| `rg35xx-sp`, `rg35xx-sp-v2` | `...rg35xx-sp(-v2-panel).dtb` | 640x480, lid | 0 | Untested |
| `rg34xx`, `rg34xx-sp`, `rg34xx-sp-v2` | `...rg34xx*.dtb` | 720x480 | 0 / 2 (SP) | Untested |
| `rg40xx-h`, `rg40xx-h-v2` | `...rg40xx-h(-v2-panel).dtb` | 640x480 | 2 | Untested |
| `rg28xx` | `...rg28xx.dtb` | 480x640 rotated | 0 | Untested |
| `rg-sp` | `...rg-sp.dtb` | 640x480, lid | 0 | Untested |

"Qualified" means the platform regression and hands-on checks passed on that
device. Every board is described the way ROCKNIX describes it (kernel patch
`0103`), and all of them share one base device tree (`rg35xx-2024.dts`).

## Hardware notes

- SoC: Allwinner H700 (4x Cortex-A53, up to 1.416 GHz in nuubOS: the 1.512 GHz
  operating point corrupts memory under load and is removed), Mali-G31 MP1 GPU
  (Mesa Panfrost), Cedrus video decoder (H.264/HEVC/MPEG-2/VP8).
- PMIC AXP717: battery gauge, charge limit (patched), power key.
- Wi-Fi/Bluetooth: Realtek RTL8821CS over SDIO (rtw88).
- RTC: PCF8563 on every board but the RG35XX 2024.
- Second card slot (TF2/SD2) on every board.
- HDMI output (dw-hdmi) up to 1080p by policy, with HDMI audio.
- USB-C: no Type-C sink resistors on the board, so some C-to-C chargers do not
  charge. A USB-A to C cable always works.

## Adding or fixing a device

1. Device tree: add or adjust it under `patches/linux/h700/` (family base
   `rg35xx-2024.dts` plus per-board files). Check buttons (`BTN_NORTH`/`BTN_WEST`
   positions), stick polarity, backlight, rumble, LED and lid against the real
   device.
2. Add the `DEVICE` name to the U-Boot selector patch and to the list in
   [storage-boot-updates.md](storage-boot-updates.md).
3. If the panel needs an init sequence, add a JSON preset in
   `board/nuubos/platform/h700/panel-firmware/presets/` and its SHA-256 in
   `scripts/generate-panel-firmware.py`.
4. Check the UI at the device's resolution with `tools/ui-shot/run.sh`.
5. Run `scripts/test-platform.sh` on the device and record the result in
   `docs/dev/platform.md`.

Platform details and history: [docs/dev/platform.md](../dev/platform.md).
