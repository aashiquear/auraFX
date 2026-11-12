#!/usr/bin/env bash
# Build the Xcode project of the auraFX iOS viewer: the Rust code as a static library plus a
# generated auraFX.xcodeproj that links, signs and packages it. Run on macOS with Xcode and
# XcodeGen (brew install xcodegen). The result is a zip to open in Xcode and sign with your
# own Apple developer account (see packaging/ios/xcode/README.md).
#   usage: scripts/package/ios_xcode.sh [out-dir]
#   IOS_BUILD_NUMBER sets the build number (default: 1); IOS_XCODE_VERIFY=1 also builds the
#   project once without signing, to prove it links.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$(mkdir -p "${1:-$ROOT/dist}" && cd "${1:-$ROOT/dist}" && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
BUILD="${IOS_BUILD_NUMBER:-1}"
WORK="$ROOT/target/package/ios-xcode"
PROJ="$WORK/auraFX-ios-xcode"
rm -rf "$WORK" && mkdir -p "$PROJ/lib"
rustup target add aarch64-apple-ios >/dev/null
export IPHONEOS_DEPLOYMENT_TARGET=15.0

# The library, and the system libraries and frameworks it needs at link time (rustc lists them).
LOG="$WORK/build.log"
cargo rustc --release -p aurafx-ios --lib --crate-type staticlib --target aarch64-apple-ios \
  --manifest-path "$ROOT/Cargo.toml" -- --print native-static-libs 2>&1 | tee "$LOG"
NATIVE="$(grep 'native-static-libs:' "$LOG" | tail -1 | sed 's/.*native-static-libs: *//')"
[ -n "$NATIVE" ] || { echo "rustc did not list the native libraries" >&2; exit 1; }
cp "$ROOT/target/aarch64-apple-ios/release/libaurafx_ios.a" "$PROJ/lib/libaurafx_ios.a"
strip -S "$PROJ/lib/libaurafx_ios.a" 2>/dev/null || true

cp -R "$ROOT/packaging/ios/xcode/." "$PROJ/"
NATIVE_ESC="$(printf '%s' "$NATIVE" | sed 's/[&|]/\\&/g')"
sed -i '' -e "s|@VERSION@|$VERSION|g" -e "s|@BUILD@|$BUILD|g" -e "s|@NATIVE_LIBS@|$NATIVE_ESC|g" "$PROJ/project.yml"
# The icon: 1024 pixels, without transparency (the App Store refuses an alpha channel).
ICON="$PROJ/Assets.xcassets/AppIcon.appiconset/icon-1024.png"
sips -z 1024 1024 "$ROOT/assets/branding/emblem.png" --out "$WORK/icon.png" >/dev/null
sips -s format jpeg "$WORK/icon.png" --out "$WORK/icon.jpg" >/dev/null
sips -s format png "$WORK/icon.jpg" --out "$ICON" >/dev/null
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/THIRD-PARTY-NOTICES.txt" "$PROJ/"
cp "$ROOT/scripts/package/ios_sign.sh" "$PROJ/"

command -v xcodegen >/dev/null || { echo "XcodeGen is needed (brew install xcodegen)" >&2; exit 1; }
(cd "$PROJ" && xcodegen generate --quiet)

(cd "$WORK" && zip -qry "$OUT/auraFX-$VERSION-ios-xcode.zip" auraFX-ios-xcode)
echo "Built $OUT/auraFX-$VERSION-ios-xcode.zip"

if [ "${IOS_XCODE_VERIFY:-0}" = "1" ]; then
  (cd "$PROJ" && xcodebuild -project auraFX.xcodeproj -scheme auraFX -configuration Release -sdk iphoneos \
     -derivedDataPath "$WORK/derived" CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build | tail -3)
  echo "The Xcode project builds."
fi
