#!/usr/bin/env bash
# Build Linux packages: a portable tarball (used by install.sh) and an AppImage.
# Both bundle a private CPython (with NumPy) so the Python console works out of the box.
#   usage: scripts/package/linux.sh [out-dir]       (set NO_APPIMAGE=1 to skip the AppImage)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$(mkdir -p "${1:-$ROOT/dist}" && cd "${1:-$ROOT/dist}" && pwd)"
ARCH="$(uname -m)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
WORK="$ROOT/target/package/linux"
rm -rf "$WORK" && mkdir -p "$WORK"

# 1. Portable Python.
PYROOT="$("$ROOT/scripts/package/fetch_python.sh" "${ARCH}-unknown-linux-gnu" "$WORK")"
"$PYROOT/bin/python3" -m pip install --quiet --disable-pip-version-check --no-warn-script-location numpy

# 2. Build against it; the binary finds libpython next to itself at run time.
export PYO3_PYTHON="$PYROOT/bin/python3"
export CARGO_TARGET_DIR="$ROOT/target/package/cargo"
# python-build-standalone reports its build-machine LIBDIR (/install/lib); link against the bundled copy.
export RUSTFLAGS="${RUSTFLAGS:-} -L native=$PYROOT/lib -C link-arg=-Wl,-rpath,\$ORIGIN/../lib/aurafx/python/lib"
cargo build --release -p aurafx-app -p aurafx-server --manifest-path "$ROOT/Cargo.toml"

# 3. AppDir layout (also used for the tarball).
APPDIR="$WORK/auraFX.AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib/aurafx" "$APPDIR/usr/share/applications"
install -m 755 "$CARGO_TARGET_DIR/release/aurafx" "$APPDIR/usr/bin/aurafx"
install -m 755 "$CARGO_TARGET_DIR/release/aurafx-server" "$APPDIR/usr/bin/aurafx-server"
cp -a "$PYROOT" "$APPDIR/usr/lib/aurafx/python"
# Trim things the embedded interpreter never needs.
rm -rf "$APPDIR/usr/lib/aurafx/python/include" "$APPDIR/usr/lib/aurafx/python/share"
find "$APPDIR/usr/lib/aurafx/python" -type d \( -name test -o -name tests -o -name __pycache__ -o -name idlelib -o -name tkinter -o -name turtledemo \) -prune -exec rm -rf {} +
for s in 16 32 48 64 128 256 512; do
  mkdir -p "$APPDIR/usr/share/icons/hicolor/${s}x${s}/apps"
  cp "$ROOT/assets/icons/aurafx-$s.png" "$APPDIR/usr/share/icons/hicolor/${s}x${s}/apps/aurafx.png"
done
cp "$ROOT/packaging/linux/aurafx.desktop" "$APPDIR/usr/share/applications/aurafx.desktop"
cp "$ROOT/packaging/linux/aurafx.desktop" "$APPDIR/aurafx.desktop"
cp "$ROOT/assets/icons/aurafx-256.png" "$APPDIR/aurafx.png"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/aurafx" "$@"
RUN
chmod +x "$APPDIR/AppRun"
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/README.md" "$APPDIR/usr/lib/aurafx/"

# Smoke test: the packaged binary starts and its Python imports NumPy.
"$APPDIR/usr/bin/aurafx" --version
PYTHONHOME="$APPDIR/usr/lib/aurafx/python" "$APPDIR/usr/lib/aurafx/python/bin/python3" -c "import numpy, sys; print('bundled python', sys.version.split()[0], 'numpy', numpy.__version__)"

# 4. Tarball.
TAR="$OUT/auraFX-$VERSION-linux-$ARCH.tar.gz"
tar -C "$WORK" -czf "$TAR" --transform "s,^auraFX.AppDir,auraFX-$VERSION," auraFX.AppDir
echo "Built $TAR"

# 5. AppImage.
if [ -z "${NO_APPIMAGE:-}" ]; then
  TOOL="$WORK/appimagetool"
  curl -fL --retry 3 -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage"
  chmod +x "$TOOL"
  ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT/auraFX-$VERSION-$ARCH.AppImage"
  echo "Built $OUT/auraFX-$VERSION-$ARCH.AppImage"
fi
