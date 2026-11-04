#!/usr/bin/env bash
# Build an unsigned iOS .ipa of the auraFX viewer (no embedded Python). Run on macOS with Xcode.
# Install it with your own Apple ID (Xcode, AltStore or Sideloadly) or sign it for TestFlight.
#   usage: scripts/package/ios.sh [out-dir]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$(mkdir -p "${1:-$ROOT/dist}" && cd "${1:-$ROOT/dist}" && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
WORK="$ROOT/target/package/ios"
rm -rf "$WORK" && mkdir -p "$WORK/Payload/auraFX.app"
rustup target add aarch64-apple-ios >/dev/null
export IPHONEOS_DEPLOYMENT_TARGET=15.0
cargo build --release -p aurafx-app --bin aurafx --no-default-features --target aarch64-apple-ios --manifest-path "$ROOT/Cargo.toml"
APP="$WORK/Payload/auraFX.app"
cp "$ROOT/target/aarch64-apple-ios/release/aurafx" "$APP/aurafx"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/ios/Info.plist" > "$APP/Info.plist"
sips -z 120 120 "$ROOT/assets/branding/emblem.png" --out "$APP/AppIcon60x60@2x.png" >/dev/null
sips -z 180 180 "$ROOT/assets/branding/emblem.png" --out "$APP/AppIcon60x60@3x.png" >/dev/null
(cd "$WORK" && zip -qry "$OUT/auraFX-$VERSION-ios-unsigned.ipa" Payload)
echo "Built $OUT/auraFX-$VERSION-ios-unsigned.ipa (unsigned)"
