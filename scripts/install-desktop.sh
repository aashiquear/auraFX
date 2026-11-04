#!/usr/bin/env bash
# Install the auraFX binary, icon and desktop entry for the current user.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/aurafx"
[ -x "$BIN" ] || { echo "Build first: cargo build --release"; exit 1; }
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"
install -m 755 "$BIN" "$HOME/.local/bin/aurafx"
for s in 16 32 48 64 128 256 512; do
  d="$HOME/.local/share/icons/hicolor/${s}x${s}/apps"
  mkdir -p "$d"
  install -m 644 "$ROOT/assets/icons/aurafx-$s.png" "$d/aurafx.png"
done
install -m 644 "$ROOT/packaging/linux/aurafx.desktop" "$HOME/.local/share/applications/aurafx.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" || true
echo "Installed. Make sure ~/.local/bin is on your PATH."
