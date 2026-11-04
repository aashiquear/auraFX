#!/usr/bin/env bash
# Download a portable CPython (python-build-standalone) and verify its checksum.
#   usage: fetch_python.sh <target-triple> <dest-dir>
#   e.g.   fetch_python.sh x86_64-unknown-linux-gnu dist/python
set -euo pipefail
TRIPLE="$1"
DEST="$2"
TAG="${PBS_TAG:-20261026}"
VER="${PBS_PYTHON:-3.12.16}"
NAME="cpython-${VER}+${TAG}-${TRIPLE}-install_only.tar.gz"
BASE="https://github.com/astral-sh/python-build-standalone/releases/download/${TAG}"
CACHE="${PBS_CACHE:-$(dirname "$DEST")/.pbs-cache}"
mkdir -p "$CACHE" "$DEST"
if [ ! -f "$CACHE/$NAME" ]; then
  curl -fL --retry 3 -o "$CACHE/$NAME.part" "$BASE/$NAME"
  mv "$CACHE/$NAME.part" "$CACHE/$NAME"
fi
curl -fsSL --retry 3 -o "$CACHE/SHA256SUMS" "$BASE/SHA256SUMS"
EXPECTED="$(grep " ${NAME}\$" "$CACHE/SHA256SUMS" | awk '{print $1}')"
if command -v sha256sum >/dev/null; then ACTUAL="$(sha256sum "$CACHE/$NAME" | awk '{print $1}')"; else ACTUAL="$(shasum -a 256 "$CACHE/$NAME" | awk '{print $1}')"; fi
if [ -z "$EXPECTED" ] || [ "$EXPECTED" != "$ACTUAL" ]; then
  echo "checksum mismatch for $NAME (expected '$EXPECTED', got '$ACTUAL')" >&2
  exit 1
fi
rm -rf "$DEST/python"
tar -xzf "$CACHE/$NAME" -C "$DEST"
echo "$DEST/python"
