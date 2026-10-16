# Licensing

nuubOS is **source-available for noncommercial use**. The source is public and you may
use, study, modify and share it for any noncommercial purpose: personal use, hobby
projects, research, education, charities and public institutions. Commercial use is
not permitted, for example selling nuubOS, preinstalling it on hardware that is sold,
or bundling it in a paid product or service.

Because it restricts commercial use, this is not an "open source" license in the OSI
sense. For a commercial license, open an issue at
<https://github.com/baf93/nuubOS/issues>.

## What the license covers

The original nuubOS work in this repository is licensed under the
[PolyForm Noncommercial License 1.0.0](LICENSE)
(SPDX: `PolyForm-Noncommercial-1.0.0`). This covers the nuubOS services, the web
administration, scripts, tools, board configuration and overlays, documentation,
translations, and the original sounds and images.

Code published in this repository before 2026-10-16 was under the MIT License.
Copies obtained under those terms keep them.

## What it does not cover

| Path / component | License |
|---|---|
| `package/nuubos/nuubui-home` (nuubUI) and `package/nuubos/nuubos-quick-menu` | GPL-3.0-only. Both are built with the [Slint](https://slint.dev) toolkit, which a console OS can use for free only under the GPL-3.0. Anyone may reuse these two programs under the GPL terms, commercial use included. |
| `patches/linux/`, `patches/u-boot/`, `patches/buildroot/`, `patches/packages/` | The license of the project each patch applies to (Linux: GPL-2.0; see each patch's SPDX lines) |
| Buildroot recipes (`*.mk`, `Config.in`, `external.*`, `configs/*_defconfig`) | GPL-2.0-or-later, like Buildroot |
| `package/nuubos/labwc` (patched labwc) | GPL-2.0-or-later |
| Third-party assets inside nuubOS packages (icons, fonts, avatars, music, OpenH264 headers) | As stated in the package's `LICENSE_FILES` and the `ATTRIBUTION`/`LICENSE` files next to them (CC-BY-4.0, CC0-1.0, OFL-1.1, BSD-2-Clause, ...) |
| Every other program in a nuubOS image (Linux, U-Boot, RetroArch, libretro cores, Syncthing, Mesa, ...) | Its own license, listed in the image's `legal-info` |

Some bundled libretro cores (FBNeo, Genesis Plus GX, MAME 2003-Plus, PicoDrive,
Snes9x and others) forbid commercial use under their own terms, independently of the
nuubOS license.

## Images and source code

Every published nuubOS image comes with its `legal-info` archive: the manifest of all
packages with their licenses, license texts, and the complete corresponding source
code of the GPL/LGPL components and the patches applied to them. The archive is
attached to the same GitHub release as the image. Third-party notices for the Rust
crates and Go modules linked into nuubOS programs are installed in the image under
`/usr/share/licenses/`.

nuubOS never ships emulator BIOS or firmware files.
