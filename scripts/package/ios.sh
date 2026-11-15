#!/usr/bin/env bash
# Build an unsigned iOS .ipa of the auraFX viewer (no embedded Python).
#   On macOS: with Xcode.
#   On Linux: with xtool (https://xtool.sh), whose Darwin SDK (`xtool setup`) holds the iOS SDK
#   and the Apple linker, and a clang (the one of the Swift toolchain xtool needs is used when
#   `clang` is not on the PATH).
# Install the .ipa with your own Apple ID: `xtool install auraFX-<version>-ios-unsigned.ipa`
# (it signs for your account), Xcode, AltStore or Sideloadly; or sign it with ios_sign.sh.
#   usage: scripts/package/ios.sh [out-dir]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$(mkdir -p "${1:-$ROOT/dist}" && cd "${1:-$ROOT/dist}" && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
WORK="$ROOT/target/package/ios"
rm -rf "$WORK" && mkdir -p "$WORK/Payload/auraFX.app"
rustup target add aarch64-apple-ios >/dev/null
export IPHONEOS_DEPLOYMENT_TARGET=15.0

if [ "$(uname -s)" != "Darwin" ]; then
  # Cross-build: the SDK and ld64 from xtool's Darwin SDK, clang from the Swift toolchain.
  BUNDLE="${DARWIN_SDK_BUNDLE:-$HOME/.swiftpm/swift-sdks/darwin.artifactbundle}"
  SDK="$BUNDLE/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"
  [ -d "$SDK" ] || { echo "no iOS SDK at $SDK: run 'xtool setup' first" >&2; exit 1; }
  TOOLS="$(dirname "$(command -v clang 2>/dev/null || echo "$HOME/.local/share/swiftly/bin/clang")")"
  [ -x "$TOOLS/clang" ] || { echo "clang is needed (the Swift toolchain has one)" >&2; exit 1; }
  AR="$(command -v llvm-ar 2>/dev/null || echo "$TOOLS/llvm-ar")"
  mkdir -p "$WORK/bin"
  for cc in clang clang++; do
    printf '#!/bin/sh\nexec "%s/%s" -target arm64-apple-ios%s -mlinker-version=1053 -isysroot "%s" -fuse-ld="%s" "$@"\n' \
      "$TOOLS" "$cc" "$IPHONEOS_DEPLOYMENT_TARGET" "$SDK" "$BUNDLE/toolset/bin/ld64.lld" > "$WORK/bin/ios-$cc"
    chmod +x "$WORK/bin/ios-$cc"
  done
  export SDKROOT="$SDK"
  export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$WORK/bin/ios-clang"
  export CC_aarch64_apple_ios="$WORK/bin/ios-clang" CXX_aarch64_apple_ios="$WORK/bin/ios-clang++" AR_aarch64_apple_ios="$AR"
  export CRATE_CC_NO_DEFAULTS=1
fi

cargo build --release -p aurafx-app --bin aurafx --no-default-features --target aarch64-apple-ios --manifest-path "$ROOT/Cargo.toml"
APP="$WORK/Payload/auraFX.app"
cp "$ROOT/target/aarch64-apple-ios/release/aurafx" "$APP/aurafx"
# See the script: without this the app is stopped at launch on iOS 27 (no scene life cycle yet).
python3 "$ROOT/scripts/package/ios_sdk_stamp.py" "$APP/aurafx" "${IOS_SDK_STAMP:-26.0}"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/ios/Info.plist" > "$APP/Info.plist"
icon() { # size file
  if command -v sips >/dev/null; then sips -z "$1" "$1" "$ROOT/assets/branding/emblem.png" --out "$2" >/dev/null
  elif command -v magick >/dev/null; then magick "$ROOT/assets/branding/emblem.png" -resize "$1x$1" "$2"
  else cp "$ROOT/assets/icons/aurafx-128.png" "$2"; fi
}
icon 120 "$APP/AppIcon60x60@2x.png"
icon 180 "$APP/AppIcon60x60@3x.png"
icon 152 "$APP/AppIcon76x76@2x~ipad.png"
icon 167 "$APP/AppIcon83.5x83.5@2x~ipad.png"
rm -f "$OUT/auraFX-$VERSION-ios-unsigned.ipa"
(cd "$WORK" && zip -qry "$OUT/auraFX-$VERSION-ios-unsigned.ipa" Payload)
echo "Built $OUT/auraFX-$VERSION-ios-unsigned.ipa (unsigned)"
