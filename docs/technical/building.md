# Building nuubOS

nuubOS is a [Buildroot](https://buildroot.org) external tree (`BR2_EXTERNAL`). The
build always runs in the versioned development container, so the host distribution
does not matter.

## Requirements

- Linux host with Docker, about 80 GB of free disk space and 16 GB of RAM.
- Network access for the first build (sources are cached in `dl/`).

## First build

```sh
git clone https://github.com/baf93/nuubOS.git
cd nuubOS
./scripts/bootstrap-buildroot.sh   # clones Buildroot 2026.08 into external/buildroot
./scripts/dev.sh                   # builds nuubos-dev:0.5 and opens a shell in it
./scripts/compile.sh               # in the container
```

The first build compiles the cross toolchain, the kernel, Mesa, Qt (for the
scraper), RetroArch and every core, so it takes several hours. Results:

| File in `output/h700/images/` | Use |
|---|---|
| `nuubOS-anbernic-h700.img` | Complete SD card image (SPL/U-Boot, slot A, boot, state) |
| `rootfs.ext4` | System slot image (the OTA payload) |
| `u-boot-sunxi-with-spl.bin` | Bootloader |
| `Image`, `*.dtb` | Kernel and device trees |

`compile.sh` applies nuubOS's Buildroot patches (`scripts/prepare-buildroot.sh`),
runs the defconfig `configs/nuubos_rg35xx_pro_defconfig` (the name is historical:
the image covers the whole H700 family) and forwards its arguments to Buildroot's
`make`. It restores Buildroot to its pristine state on exit.

Without the interactive shell:

```sh
docker run --rm --user "$(id -u):$(id -g)" -e HOME=/workspace/.docker-home \
  -e BR2_DL_DIR=/workspace/dl -e BR2_CCACHE_DIR=/workspace/.cache/nuubos/buildroot-ccache \
  -v "$PWD:/workspace" -w /workspace nuubos-dev:0.5 ./scripts/compile.sh
```

## Incremental builds

Rebuild only the packages you changed, then regenerate the image:

```sh
./scripts/compile.sh nuubui-home-rebuild     # one package
./scripts/compile.sh                         # image only: target-finalize, rootfs, post-image
```

Rules that keep builds fast and reproducible:

- Never call Buildroot's `make` directly; always go through `compile.sh`.
- Do not delete `output/` or use `compile.sh --clean` unless a clean build is
  really needed (rootfs composition, toolchain or global configuration changes).
- Several `-rebuild` targets in one call can skip a package that is also a
  dependency of an earlier one. Check for its `>>> <pkg> Building` line, or
  rebuild it alone.
- Never run two builds on the same `output/` at the same time.

More gotchas (kernel, Mesa, wlroots, cargo cache): [docs/dev/build-deploy.md](../dev/build-deploy.md).

## Kernel and bootloader

- Kernel patches: `patches/linux/h700/` and `patches/linux/device/`; configuration
  `board/nuubos/platform/h700/linux.config`. `./scripts/compile.sh linux-rebuild`.
- U-Boot: `patches/u-boot/`, fragment `board/nuubos/platform/h700/uboot.fragment`.
  The device selector reads `DEVICE` from `nuubos.cfg` on `NUUBOS_BOOT` and picks the
  device tree.
- TF-A: `patches/packages/arm-trusted-firmware/` (adds deep sleep for the H616/H700).

## Tests

| What | How |
|---|---|
| Service host tests | `python3 package/nuubos/<pkg>/tests/*.py` or `sh .../tests/*.sh` (fake roots, fake peers) |
| Rust unit tests | `cargo test` in `package/nuubos/nuubos-remote/src` |
| UI screenshots without hardware | `tools/ui-shot/run.sh [lang ...]` renders the scenes at 640x480, 720x720, 720x480, 1280x720 and 1920x1080 into `output/ui-shot/<lang>/`; `ONLY=<prefix>` for a subset |
| Translations | `sh validate-i18n.sh .` in `package/nuubos/nuubos-localization/src/i18n` |
| Platform regression on a device | `scripts/test-platform.sh` |

## Working on a device

Development happens on a running console instead of reflashing:

- `tools/deploy-target.sh -H root@<ip> [-s <init script>] <files...>` copies files
  from `output/h700/target`, backs up the originals and restarts services.
- `tools/grab-target.sh <name>` takes a screenshot of the real screen.
- SSH is enabled in Settings → Connectivity → Services; the password is shown there.

## License material

Every published image needs its `legal-info`: run `./scripts/compile.sh legal-info`
after the image build. Third-party notices for Rust crates and Go modules are
generated into the image at build time (`/usr/share/licenses/<pkg>/`). See
[LICENSING.md](../../LICENSING.md) and [releasing.md](releasing.md).
