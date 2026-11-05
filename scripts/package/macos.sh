#!/usr/bin/env bash
# Build auraFX.app (with a bundled CPython + NumPy) and a .dmg. Run on macOS.
#   usage: scripts/package/macos.sh [out-dir]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$(mkdir -p "${1:-$ROOT/dist}" && cd "${1:-$ROOT/dist}" && pwd)"
ARCH="$(uname -m)"; [ "$ARCH" = "arm64" ] && TRIPLE_ARCH=aarch64 || TRIPLE_ARCH=x86_64
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
WORK="$ROOT/target/package/macos"
rm -rf "$WORK" && mkdir -p "$WORK"

PYROOT="$("$ROOT/scripts/package/fetch_python.sh" "${TRIPLE_ARCH}-apple-darwin" "$WORK")"
"$PYROOT/bin/python3" -m pip install --quiet --disable-pip-version-check --no-warn-script-location numpy

export PYO3_PYTHON="$PYROOT/bin/python3"
export CARGO_TARGET_DIR="$ROOT/target/package/cargo-macos"
export MACOSX_DEPLOYMENT_TARGET=11.0
export RUSTFLAGS="${RUSTFLAGS:-} -L native=$PYROOT/lib -C link-arg=-Wl,-rpath,@executable_path/../Resources/python/lib"
cargo build --release -p aurafx-app -p aurafx-server --manifest-path "$ROOT/Cargo.toml"

APP="$WORK/auraFX.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m 755 "$CARGO_TARGET_DIR/release/aurafx" "$APP/Contents/MacOS/aurafx"
install -m 755 "$CARGO_TARGET_DIR/release/aurafx-server" "$APP/Contents/MacOS/aurafx-server"
# Make sure the binary refers to libpython through @rpath.
OLD="$(otool -L "$APP/Contents/MacOS/aurafx" | awk '/libpython/ {print $1}')"
if [ -n "$OLD" ] && [ "${OLD#@rpath}" = "$OLD" ]; then
  install_name_tool -change "$OLD" "@rpath/$(basename "$OLD")" "$APP/Contents/MacOS/aurafx"
fi
cp -a "$PYROOT" "$APP/Contents/Resources/python"
rm -rf "$APP/Contents/Resources/python/include" "$APP/Contents/Resources/python/share"
find "$APP/Contents/Resources/python" -type d \( -name test -o -name tests -o -name __pycache__ -o -name idlelib -o -name tkinter -o -name turtledemo \) -prune -exec rm -rf {} +
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/Info.plist" > "$APP/Contents/Info.plist"

# Icon.
ICONSET="$WORK/aurafx.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$ROOT/assets/branding/emblem.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); sips -z $d $d "$ROOT/assets/branding/emblem.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/aurafx.icns"

# Ad-hoc signature (required on Apple silicon). Set MACOS_SIGN_ID for a Developer ID.
codesign --force --deep --sign "${MACOS_SIGN_ID:--}" "$APP"
"$APP/Contents/MacOS/aurafx" --version

DMG_DIR="$WORK/dmg"
mkdir -p "$DMG_DIR"
cp -a "$APP" "$DMG_DIR/"
ln -s /Applications "$DMG_DIR/Applications"
hdiutil create -volname "auraFX" -srcfolder "$DMG_DIR" -ov -format UDZO "$OUT/auraFX-$VERSION-macos-$ARCH.dmg"
(cd "$WORK" && zip -qry "$OUT/auraFX-$VERSION-macos-$ARCH.app.zip" auraFX.app)
echo "Built $OUT/auraFX-$VERSION-macos-$ARCH.dmg"
