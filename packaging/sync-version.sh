#!/bin/sh
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
METAINFO="$ROOT/packaging/io.github.AeternalAngel.Aeternus.metainfo.xml"
PKGS="aeternus aeternus-bin"
TAB=$(printf '\t')

CHECK=0
case "${1:-}" in
    --check) CHECK=1 ;;
    "") ;;
    *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

V=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
[ -n "$V" ] || { echo "no version in Cargo.toml" >&2; exit 1; }

case "$V" in
    *-*)
        echo "Cargo.toml is at pre-release $V; AUR and AppStream stay on the last stable release."
        exit 0
        ;;
esac

if [ "$CHECK" = 1 ]; then
    FAIL=0
    for pkg in $PKGS; do
        D="$ROOT/packaging/aur/$pkg"
        P=$(sed -n 's/^pkgver=//p' "$D/PKGBUILD")
        S=$(sed -n "s/^${TAB}pkgver = //p" "$D/.SRCINFO")
        [ "$P" = "$V" ] || { echo "$pkg/PKGBUILD pkgver is $P, Cargo.toml is $V"; FAIL=1; }
        [ "$S" = "$V" ] || { echo "$pkg/.SRCINFO pkgver is $S, Cargo.toml is $V"; FAIL=1; }
        if grep "^${TAB}source" "$D/.SRCINFO" | grep -qv "/v$V[/.]"; then
            echo "$pkg/.SRCINFO sources do not point at v$V"
            FAIL=1
        fi
    done
    FIRST=$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' "$METAINFO" | head -n 1)
    [ "$FIRST" = "$V" ] || { echo "newest AppStream release is $FIRST, Cargo.toml is $V"; FAIL=1; }
    [ "$FAIL" = 0 ] && echo "AUR and AppStream match $V."
    exit "$FAIL"
fi

command -v makepkg > /dev/null || { echo "makepkg is needed to regenerate .SRCINFO" >&2; exit 1; }

for pkg in $PKGS; do
    D="$ROOT/packaging/aur/$pkg"
    P=$(sed -n 's/^pkgver=//p' "$D/PKGBUILD")
    if [ "$P" != "$V" ]; then
        sed -i -e "s/^pkgver=.*/pkgver=$V/" -e 's/^pkgrel=.*/pkgrel=1/' \
            -e "s/^\(sha256sums[_a-z0-9]*\)=(.*)\$/\1=('SKIP')/" "$D/PKGBUILD"
        echo "$pkg: $P -> $V"
    fi
    (cd "$D" && makepkg --printsrcinfo > .SRCINFO)
done

if ! grep -q "<release version=\"$V\"" "$METAINFO"; then
    SHORT=$(echo "$V" | sed -E 's/^([0-9]+\.[0-9]+)\.0$/\1/')
    DATE=$(sed -n "s/^## \[$SHORT\] - \([0-9-]*\).*/\1/p" "$ROOT/CHANGELOG.md" | head -n 1)
    [ -n "$DATE" ] || DATE=$(date +%Y-%m-%d)
    sed -i "s|^  <releases>\$|  <releases>\n    <release version=\"$V\" date=\"$DATE\" />|" "$METAINFO"
    echo "AppStream: added release $V ($DATE); consider adding a <description>."
fi
