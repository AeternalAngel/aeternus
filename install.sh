#!/bin/sh
set -eu

APP_ID=io.github.AeternalAngel.Aeternus
PREFIX=${PREFIX:-"$HOME/.local"}
DESTDIR=${DESTDIR:-}
ACTION=install

usage() {
    cat <<EOF
usage: ./install.sh [--prefix DIR] [--uninstall]

Installs aeternus, aet, the desktop entry, icons, man pages, shell completions and the
xterm-aeternus terminfo entry.
From a source checkout it runs 'cargo build --release' first.

  --prefix DIR   install under DIR (default: \$HOME/.local; use /usr/local for a system install)
  --uninstall    remove the files installed under the prefix
  -h, --help     show this help

DESTDIR is honored for packaging.
EOF
}

while [ $# -gt 0 ]; do
    case $1 in
        --prefix) PREFIX=$2; shift 2 ;;
        --prefix=*) PREFIX=${1#--prefix=}; shift ;;
        --uninstall) ACTION=uninstall; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "install.sh: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT="$DESTDIR$PREFIX"

build_from_source() {
    [ -f "$HERE/Cargo.toml" ] || return 0
    if [ "$(id -u)" = 0 ] && [ -x "$HERE/target/release/aeternus" ] && [ -x "$HERE/target/release/aet" ]; then
        return 0
    fi
    if [ "$(id -u)" = 0 ]; then
        echo "install.sh: build first as your normal user ('cargo build --release'), then run this with sudo" >&2
        exit 1
    fi
    if ! command -v cargo >/dev/null 2>&1; then
        echo "install.sh: cargo not found; install Rust from https://rustup.rs and run this again" >&2
        exit 1
    fi
    echo "Building Aeternus (release)..."
    (cd "$HERE" && cargo build --release --locked)
}

find_bin() {
    for p in "$HERE/target/release/$1" "$HERE/$1"; do
        if [ -x "$p" ]; then
            echo "$p"
            return
        fi
    done
    echo "install.sh: $1 not found; run 'cargo build --release' first" >&2
    exit 1
}

if [ -d "$HERE/packaging" ]; then
    PKG="$HERE/packaging"
else
    PKG="$HERE"
fi

FILES="
bin/aeternus
bin/aet
share/applications/$APP_ID.desktop
share/metainfo/$APP_ID.metainfo.xml
share/man/man1/aeternus.1
share/man/man1/aet.1
share/bash-completion/completions/aeternus
share/bash-completion/completions/aet
share/zsh/site-functions/_aeternus
share/zsh/site-functions/_aet
share/fish/vendor_completions.d/aeternus.fish
share/fish/vendor_completions.d/aet.fish
share/terminfo/x/xterm-aeternus
"
SIZES="16x16 32x32 48x48 64x64 128x128 256x256 512x512"

refresh() {
    [ -n "$DESTDIR" ] && return 0
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || :
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f "$PREFIX/share/icons/hicolor" 2>/dev/null || :
    fi
}

install_terminfo() {
    src="$PKG/terminfo/xterm-aeternus.terminfo"
    if command -v tic >/dev/null 2>&1 && tic -x -o "$ROOT/share/terminfo" "$src" 2>/dev/null; then
        return 0
    fi
    if [ -f "$PKG/terminfo/x/xterm-aeternus" ]; then
        install -Dm644 "$PKG/terminfo/x/xterm-aeternus" "$ROOT/share/terminfo/x/xterm-aeternus"
        return 0
    fi
    echo "note: tic not found, terminfo entry not installed; the shell will use TERM=xterm-256color"
}

remove_legacy() {
    legacy="$ROOT/share/applications/aeternus.desktop"
    if [ -f "$legacy" ] && grep -q '^Name=Aeternus$' "$legacy"; then
        rm -f "$legacy"
        echo "removed old menu entry $legacy"
    fi
    for s in $SIZES; do
        rm -f "$ROOT/share/icons/hicolor/$s/apps/aeternus.png"
    done
    rm -f "$ROOT/share/icons/hicolor/scalable/apps/aeternus.svg"
}

if [ "$ACTION" = uninstall ]; then
    for f in $FILES; do
        rm -f "$ROOT/$f"
    done
    rmdir "$ROOT/share/terminfo/x" "$ROOT/share/terminfo" 2>/dev/null || :
    for s in $SIZES; do
        rm -f "$ROOT/share/icons/hicolor/$s/apps/$APP_ID.png"
    done
    remove_legacy
    refresh
    echo "Aeternus removed from $ROOT"
    exit 0
fi

build_from_source
AETERNUS=$(find_bin aeternus)
AET=$(find_bin aet)

install -Dm755 "$AETERNUS" "$ROOT/bin/aeternus"
install -Dm755 "$AET" "$ROOT/bin/aet"
mkdir -p "$ROOT/share/applications"
sed "s|^Exec=aeternus|Exec=$PREFIX/bin/aeternus|" "$PKG/$APP_ID.desktop" > "$ROOT/share/applications/$APP_ID.desktop"
chmod 644 "$ROOT/share/applications/$APP_ID.desktop"
install -Dm644 "$PKG/$APP_ID.metainfo.xml" "$ROOT/share/metainfo/$APP_ID.metainfo.xml"
for s in $SIZES; do
    install -Dm644 "$PKG/icons/hicolor/$s/apps/$APP_ID.png" "$ROOT/share/icons/hicolor/$s/apps/$APP_ID.png"
done
install -Dm644 "$PKG/man/aeternus.1" "$ROOT/share/man/man1/aeternus.1"
install -Dm644 "$PKG/man/aet.1" "$ROOT/share/man/man1/aet.1"
install -Dm644 "$PKG/completions/aeternus.bash" "$ROOT/share/bash-completion/completions/aeternus"
install -Dm644 "$PKG/completions/aet.bash" "$ROOT/share/bash-completion/completions/aet"
install -Dm644 "$PKG/completions/_aeternus" "$ROOT/share/zsh/site-functions/_aeternus"
install -Dm644 "$PKG/completions/_aet" "$ROOT/share/zsh/site-functions/_aet"
install -Dm644 "$PKG/completions/aeternus.fish" "$ROOT/share/fish/vendor_completions.d/aeternus.fish"
install -Dm644 "$PKG/completions/aet.fish" "$ROOT/share/fish/vendor_completions.d/aet.fish"
install_terminfo
remove_legacy
refresh

echo "Aeternus installed to $ROOT"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "note: $PREFIX/bin is not in your PATH" ;;
esac
if [ -z "$DESTDIR" ]; then
    found=$(command -v aeternus 2>/dev/null || :)
    if [ -n "$found" ] && [ "$found" != "$PREFIX/bin/aeternus" ]; then
        echo "warning: $found comes first in your PATH and runs instead of the new install"
        echo "         ($("$found" --version 2>/dev/null || echo unknown version)); remove it, e.g. 'cargo uninstall aeternus'"
    fi
fi
