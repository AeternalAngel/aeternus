#!/bin/sh
# Refreshes the checksums and .SRCINFO of both AUR packages.
#
# Run it only after the v<pkgver> release is published on GitHub: updpkgsums
# downloads the tag tarball (aeternus) and the release tarballs (aeternus-bin)
# to hash them. Needs makepkg and updpkgsums (pacman-contrib).
#
#   packaging/aur/update.sh
#
# Then review the diff, commit it, and copy each directory's PKGBUILD and
# .SRCINFO into its AUR git clone before pushing there.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)

for pkg in aeternus aeternus-bin; do
    echo "==> $pkg"
    (
        cd "$HERE/$pkg"
        updpkgsums
        makepkg --printsrcinfo > .SRCINFO
    )
done
