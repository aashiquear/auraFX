#!/usr/bin/env bash
# Build the auraFX Android APK (arm64-v8a) without Gradle.
#   Needs: ANDROID_HOME (SDK with build-tools and platforms/android-35), ANDROID_NDK_HOME, cargo-ndk.
#   Signing: set ANDROID_KEYSTORE, ANDROID_KEYSTORE_PASS, ANDROID_KEY_ALIAS for a release key;
#            otherwise a local debug key is generated.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/dist}"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
: "${ANDROID_HOME:?set ANDROID_HOME}"
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME}"
BT="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
PLATFORM="$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cd "$ROOT"
ABIS="${ANDROID_ABIS:-arm64-v8a x86_64}"
TARGETS=""
for a in $ABIS; do TARGETS="$TARGETS -t $a"; done
# Kept in target/android-libs: scripts/package/android_studio.sh reuses them.
LIBS="$ROOT/target/android-libs"
cargo ndk $TARGETS -o "$LIBS" build -p aurafx-android --release

# Resources: launcher icons.
for pair in mdpi:48 hdpi:72 xhdpi:96 xxhdpi:144 xxxhdpi:192; do
  d="${pair%%:*}"; s="${pair##*:}"
  mkdir -p "$WORK/res/mipmap-$d"
  if command -v magick >/dev/null; then
    magick "$ROOT/assets/branding/emblem.png" -resize "${s}x${s}" "$WORK/res/mipmap-$d/ic_launcher.png"
  else
    cp "$ROOT/assets/icons/aurafx-128.png" "$WORK/res/mipmap-$d/ic_launcher.png"
  fi
done
CODE=$(echo "$VERSION" | awk -F. '{print $1*10000 + $2*100 + $3}')
sed -e "s/@VERSION_CODE@/$CODE/" -e "s/@VERSION_NAME@/$VERSION/" "$ROOT/packaging/android/AndroidManifest.xml" > "$WORK/AndroidManifest.xml"

"$BT/aapt2" compile --dir "$WORK/res" -o "$WORK/res.zip"
"$BT/aapt2" link -o "$WORK/base.apk" -I "$PLATFORM/android.jar" --manifest "$WORK/AndroidManifest.xml" -R "$WORK/res.zip" --auto-add-overlay

# Add the native library.
for a in $ABIS; do
  mkdir -p "$WORK/apk/lib/$a"
  cp "$LIBS/$a/libaurafx.so" "$WORK/apk/lib/$a/"
done
python3 - "$WORK/base.apk" "$WORK/apk" <<'PY'
import os, sys, zipfile
apk, root = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(apk, "a", zipfile.ZIP_DEFLATED) as z:
    for d, _, files in os.walk(root):
        for f in files:
            p = os.path.join(d, f)
            z.write(p, os.path.relpath(p, root))
PY
"$BT/zipalign" -p -f 4 "$WORK/base.apk" "$WORK/aligned.apk"

if [ -n "${ANDROID_KEYSTORE:-}" ]; then
  KS="$ANDROID_KEYSTORE"; PASS="$ANDROID_KEYSTORE_PASS"; ALIAS="${ANDROID_KEY_ALIAS:-aurafx}"
else
  # Reuse the standard Android debug key so updates install over earlier builds.
  KS="$HOME/.android/debug.keystore"; PASS="android"; ALIAS="androiddebugkey"
  mkdir -p "$HOME/.android"
  [ -f "$KS" ] || keytool -genkeypair -keystore "$KS" -storepass "$PASS" -keypass "$PASS" -alias "$ALIAS" -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=auraFX Debug" >/dev/null 2>&1
fi
mkdir -p "$OUT"
"$BT/apksigner" sign --ks "$KS" --ks-pass "pass:$PASS" --ks-key-alias "$ALIAS" --out "$OUT/auraFX-$VERSION-android.apk" "$WORK/aligned.apk"
"$BT/apksigner" verify "$OUT/auraFX-$VERSION-android.apk"
echo "Built $OUT/auraFX-$VERSION-android.apk"
