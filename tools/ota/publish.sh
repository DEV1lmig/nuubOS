#!/bin/sh
# Publish an OTA release made by tools/ota/make-update.sh on GitHub (EPIC-048).
#
#   tools/ota/publish.sh OUTDIR
#
# Needs the GitHub CLI (gh), logged in with write access to the repository.
# 1. release <tag> (versioned, kept) with every file in OUTDIR/<tag>/ (the
#    payload, plus the image, legal-info, notices and SHA256SUMS when
#    tools/ota/add-image.sh added them), at the current commit (pushed
#    first); a pre-release unless the channel is stable;
# 2. release channel-<channel> (created once, a pre-release so it is never
#    "Latest"): its manifest and signature are replaced. Devices on that
#    channel see the new version at their next check.
# The payload goes up before the manifest that points to it.

REPO="${NUUBOS_REPO:-baf93/nuubOS}"
OUT="${1:-}"
[ -f "$OUT/release.env" ] || { echo "Usage: $0 OUTDIR (made by tools/ota/make-update.sh)" >&2; exit 2; }
command -v gh >/dev/null || { echo "[FAIL] GitHub CLI (gh) not installed" >&2; exit 1; }
. "$OUT/release.env"
M="$OUT/channel-$CHANNEL/nuubos-update.manifest"
[ -f "$OUT/$TAG/$PAYLOAD" ] && [ -f "$M" ] && [ -f "$M.sig" ] || { echo "[FAIL] $OUT is incomplete" >&2; exit 1; }
COMMIT="$(git -C "$(dirname "$0")/../.." rev-parse HEAD)"
PRE=""; [ "$CHANNEL" = stable ] || PRE="--prerelease"

set -- "$OUT/$TAG"/*
ASSETS="$*"
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
    # shellcheck disable=SC2086
    gh release upload "$TAG" $ASSETS --repo "$REPO" --clobber || exit 1
else
    # github-notes.md (optional, e.g. flashing steps) is the GitHub text;
    # else the console's release notes.
    if [ -f "$OUT/github-notes.md" ]; then set -- --notes-file "$OUT/github-notes.md"
    elif [ -f "$OUT/release-notes.txt" ]; then set -- --notes-file "$OUT/release-notes.txt"
    else set -- --notes "nuubOS $VERSION, $CHANNEL channel."; fi
    # shellcheck disable=SC2086
    gh release create "$TAG" $ASSETS --repo "$REPO" --target "$COMMIT" $PRE \
        --title "nuubOS $VERSION" "$@" || exit 1
fi
if ! gh release view "channel-$CHANNEL" --repo "$REPO" >/dev/null 2>&1; then
    gh release create "channel-$CHANNEL" --repo "$REPO" --target "$COMMIT" --prerelease --latest=false \
        --title "Update channel: $CHANNEL" \
        --notes "OTA pointer for nuubOS devices on the $CHANNEL channel: always the current signed manifest. Not a download." || exit 1
fi
gh release upload "channel-$CHANNEL" "$M" "$M.sig" --repo "$REPO" --clobber || exit 1
echo "[PASS] $TAG published, channel $CHANNEL points to it"
