#!/bin/sh
# auraFX installer for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/aashiquear/auraFX/main/scripts/install.sh | sh
#
# Options (pass after `sh -s --` when piping):
#   --version vX.Y.Z   install a specific release (default: latest)
#   --file PATH        install from a downloaded package (.tar.gz on Linux, .dmg on macOS)
#   --prefix DIR       Linux install location (default: ~/.local)
#   --uninstall        remove auraFX
set -eu

REPO="aashiquear/auraFX"
VERSION="latest"
FILE=""
PREFIX="${AURAFX_PREFIX:-$HOME/.local}"
UNINSTALL=0
while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --file) FILE="$2"; shift 2 ;;
    --prefix) PREFIX="$2"; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done

say() { printf '\033[1;33mauraFX:\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mauraFX:\033[0m %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "this installer needs '$1'"; }

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$ARCH" in arm64|aarch64) ARCH_LINUX=aarch64; ARCH_MAC=arm64 ;; x86_64|amd64) ARCH_LINUX=x86_64; ARCH_MAC=x86_64 ;; *) die "unsupported CPU $ARCH" ;; esac

DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"

if [ "$UNINSTALL" = 1 ]; then
  if [ "$OS" = "Darwin" ]; then
    rm -rf "/Applications/auraFX.app" "$HOME/Applications/auraFX.app"
  else
    rm -rf "$PREFIX/opt/aurafx" "$PREFIX/bin/aurafx" "$PREFIX/bin/aurafx-server" "$DATA_HOME/applications/aurafx.desktop"
    find "$DATA_HOME/icons/hicolor" -name aurafx.png -delete 2>/dev/null || true
  fi
  say "removed"
  exit 0
fi

# Resolve the download URL for an asset whose name ends with $1.
asset_url() {
  need curl
  if [ "$VERSION" = "latest" ]; then api="https://api.github.com/repos/$REPO/releases/latest"; else api="https://api.github.com/repos/$REPO/releases/tags/$VERSION"; fi
  curl -fsSL "$api" | grep '"browser_download_url"' | grep -- "$1\"" | head -n 1 | sed 's/.*"\(https[^"]*\)".*/\1/'
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if [ "$OS" = "Linux" ]; then
  if [ -z "$FILE" ]; then
    URL="$(asset_url "-linux-$ARCH_LINUX.tar.gz")"
    [ -n "$URL" ] || die "no Linux $ARCH_LINUX package found for release '$VERSION'"
    say "downloading $URL"
    curl -fL --progress-bar -o "$TMP/aurafx.tar.gz" "$URL"
    FILE="$TMP/aurafx.tar.gz"
  fi
  need tar
  mkdir -p "$PREFIX/opt" "$PREFIX/bin"
  rm -rf "$PREFIX/opt/aurafx"
  mkdir -p "$TMP/x"
  tar -xzf "$FILE" -C "$TMP/x"
  mv "$TMP/x"/* "$PREFIX/opt/aurafx"
  ln -sf "$PREFIX/opt/aurafx/usr/bin/aurafx" "$PREFIX/bin/aurafx"
  # The data server, so other auraFX apps can open files on this machine over ssh.
  [ -x "$PREFIX/opt/aurafx/usr/bin/aurafx-server" ] && ln -sf "$PREFIX/opt/aurafx/usr/bin/aurafx-server" "$PREFIX/bin/aurafx-server"
  mkdir -p "$DATA_HOME/applications"
  sed "s|^Exec=.*|Exec=$PREFIX/opt/aurafx/usr/bin/aurafx %F|" "$PREFIX/opt/aurafx/usr/share/applications/aurafx.desktop" > "$DATA_HOME/applications/aurafx.desktop"
  for d in "$PREFIX/opt/aurafx/usr/share/icons/hicolor"/*; do
    s="$(basename "$d")"
    mkdir -p "$DATA_HOME/icons/hicolor/$s/apps"
    cp "$d/apps/aurafx.png" "$DATA_HOME/icons/hicolor/$s/apps/aurafx.png"
  done
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DATA_HOME/applications" >/dev/null 2>&1 || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$DATA_HOME/icons/hicolor" >/dev/null 2>&1 || true
  "$PREFIX/bin/aurafx" --version >/dev/null || die "installed, but the binary does not start"
  say "installed to $PREFIX/opt/aurafx"
  case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) say "add $PREFIX/bin to your PATH to run 'aurafx' from a terminal" ;; esac
  say "start it from your application menu or run: aurafx"
elif [ "$OS" = "Darwin" ]; then
  if [ -z "$FILE" ]; then
    URL="$(asset_url "-macos-$ARCH_MAC.dmg")"
    [ -n "$URL" ] || die "no macOS $ARCH_MAC package found for release '$VERSION'"
    say "downloading $URL"
    curl -fL --progress-bar -o "$TMP/aurafx.dmg" "$URL"
    FILE="$TMP/aurafx.dmg"
  fi
  MNT="$TMP/mnt"
  mkdir -p "$MNT"
  hdiutil attach -nobrowse -quiet -mountpoint "$MNT" "$FILE"
  DEST="/Applications"
  [ -w "$DEST" ] || { DEST="$HOME/Applications"; mkdir -p "$DEST"; }
  rm -rf "$DEST/auraFX.app"
  cp -R "$MNT/auraFX.app" "$DEST/"
  hdiutil detach -quiet "$MNT"
  xattr -dr com.apple.quarantine "$DEST/auraFX.app" 2>/dev/null || true
  say "installed to $DEST/auraFX.app"
else
  die "unsupported OS $OS (on Windows use scripts/install.ps1)"
fi
