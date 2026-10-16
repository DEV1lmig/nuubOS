# Releasing

How the maintainer publishes a nuubOS image and its over-the-air update. The same
steps serve the Nightly, RC and Stable channels.

## Before you start

- The tree is committed and pushed: the image records `git describe`, and a
  `-dirty` build must not be published.
- No other build runs on `output/`.
- GitHub CLI installed and signed in with write access: `gh auth status`.
- The release passphrase is at hand (the maintainer's password manager).

## 1. Build the image for the channel

The device's default channel is compiled into `nuubos-update`. Build it for the
channel you publish on, then the image:

```sh
./scripts/compile.sh NUUBOS_UPDATE_DEFAULT_CHANNEL=nightly nuubos-update-rebuild
./scripts/compile.sh
./scripts/compile.sh legal-info
```

(in the development container, see [building.md](building.md)). The build serial
(`/usr/share/nuubos/build-serial`) is set when the rootfs is generated; it orders
releases on the devices.

## 2. Qualify

Flash `output/h700/images/nuubOS-anbernic-h700.img` on a spare card and check on at
least one device: first boot and setup wizard, a game, sleep and wake, Wi-Fi,
Settings → System → Software Update shows the right version and channel. For an RC
or Stable release, run the platform regression (`scripts/test-platform.sh`) on every
qualified device.

## 3. Create the release files

```sh
tools/ota/make-update.sh ~/nuubos-release nightly ~/nuubos-release-notes.txt
tools/ota/add-image.sh ~/nuubos-release
```

- `make-update.sh` asks for the passphrase, checks it against `update-key.pub`,
  compresses `rootfs.ext4` into the payload and signs the manifest. The notes file
  is plain text, at most 4000 bytes, one item per line: it appears on the console
  under Software Update → What's New.
- `add-image.sh` adds the flashable `.img.xz`, `legal-info` (split into 1900 MB
  parts, below GitHub's 2 GiB asset limit), the third-party notices and
  `SHA256SUMS`.
- Optional: `~/nuubos-release/github-notes.md` becomes the GitHub release text
  (installation steps, checksums). Without it, the console notes are used.

## 4. Publish

```sh
git push
tools/ota/publish.sh ~/nuubos-release
```

`publish.sh` creates the versioned release `v<version>-<serial>` with every file
(a pre-release unless the channel is stable), then replaces the manifest and
signature in `channel-<channel>`. Devices on that channel see the update at their
next check (within a day, or at once with Check).

## 5. After publishing

- Check from a device on the channel: Settings → System → Software Update → Check.
- Update the wiki pages that changed (Known Issues, Release Notes).

## Rejoining legal-info

```sh
cat nuubOS-<version>-<serial>-legal-info.tar.part* | tar -xf -
```

## Versions and channels

- `VERSION` holds the product version (`0.5`). The tag adds the build serial:
  `v0.5-1792000000`.
- Nightly: Alpha builds for testers, any time.
- RC: release candidates, bug fixes only.
- Stable: after RC qualification on every supported device.
