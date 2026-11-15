#!/usr/bin/env bash
# Sign the unsigned auraFX .ipa with your own Apple developer identity. Run on macOS.
#   usage: ios_sign.sh auraFX-<version>-ios-unsigned.ipa "Apple Development: Name (TEAMID)" profile.mobileprovision [out.ipa]
# The identity is one from your keychain (`security find-identity -v -p codesigning` lists
# them); the provisioning profile must cover the app's bundle identifier (org.aurafx.app, or
# pass BUNDLE_ID=your.id to change it) and, for a development or ad hoc profile, your device.
set -euo pipefail
[ $# -ge 3 ] || { sed -n '2,6p' "$0"; exit 2; }
IPA="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; IDENTITY="$2"
PROFILE="$(cd "$(dirname "$3")" && pwd)/$(basename "$3")"
OUT="${4:-${IPA%-unsigned.ipa}-signed.ipa}"
case "$OUT" in /*) ;; *) OUT="$PWD/$OUT" ;; esac
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
(cd "$WORK" && unzip -q "$IPA")
APP="$(ls -d "$WORK"/Payload/*.app | head -1)"
if [ -n "${BUNDLE_ID:-}" ]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID" "$APP/Info.plist"
fi
cp "$PROFILE" "$APP/embedded.mobileprovision"
# The entitlements the profile grants.
security cms -D -i "$PROFILE" > "$WORK/profile.plist"
/usr/libexec/PlistBuddy -x -c "Print :Entitlements" "$WORK/profile.plist" > "$WORK/entitlements.plist"
rm -rf "$APP/_CodeSignature"
codesign --force --sign "$IDENTITY" --entitlements "$WORK/entitlements.plist" --timestamp=none "$APP"
codesign --verify --verbose=2 "$APP"
rm -f "$OUT"; (cd "$WORK" && zip -qry "$OUT" Payload)
echo "Signed: $OUT"
