#!/usr/bin/env bash
# Build the Android Studio (Gradle) project of the auraFX viewer: the template in
# packaging/android/studio with the native libraries, icons and version filled in, as a zip to
# open in Android Studio and build a signed app bundle for the Play Console from
# (see packaging/android/studio/README.md).
#   Needs: ANDROID_NDK_HOME and cargo-ndk, unless target/android-libs already holds the
#   libraries (scripts/package/android_apk.sh leaves them there).
#   ANDROID_VERSION_CODE overrides the version code; ANDROID_STUDIO_VERIFY=1 also runs
#   `gradle bundleRelease` once (needs ANDROID_HOME and Gradle) to prove the project builds.
#   usage: scripts/package/android_studio.sh [out-dir]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$(mkdir -p "${1:-$ROOT/dist}" && cd "${1:-$ROOT/dist}" && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
CODE="${ANDROID_VERSION_CODE:-$(echo "$VERSION" | awk -F. '{print $1*10000 + $2*100 + $3}')}"
ABIS="${ANDROID_ABIS:-arm64-v8a x86_64}"
LIBS="$ROOT/target/android-libs"
WORK="$ROOT/target/package/android-studio"
PROJ="$WORK/auraFX-android-studio"
rm -rf "$WORK" && mkdir -p "$WORK"
cp -R "$ROOT/packaging/android/studio" "$PROJ"

have=1; for a in $ABIS; do [ -f "$LIBS/$a/libaurafx.so" ] || have=0; done
if [ "$have" = 0 ]; then
  TARGETS=""; for a in $ABIS; do TARGETS="$TARGETS -t $a"; done
  (cd "$ROOT" && cargo ndk $TARGETS -o "$LIBS" build -p aurafx-android --release)
fi
for a in $ABIS; do
  mkdir -p "$PROJ/app/src/main/jniLibs/$a"
  cp "$LIBS/$a/libaurafx.so" "$PROJ/app/src/main/jniLibs/$a/"
done
for pair in mdpi:48 hdpi:72 xhdpi:96 xxhdpi:144 xxxhdpi:192; do
  d="${pair%%:*}"; s="${pair##*:}"
  mkdir -p "$PROJ/app/src/main/res/mipmap-$d"
  if command -v magick >/dev/null; then
    magick "$ROOT/assets/branding/emblem.png" -resize "${s}x${s}" "$PROJ/app/src/main/res/mipmap-$d/ic_launcher.png"
  elif command -v convert >/dev/null; then
    convert "$ROOT/assets/branding/emblem.png" -resize "${s}x${s}" "$PROJ/app/src/main/res/mipmap-$d/ic_launcher.png"
  else
    cp "$ROOT/assets/icons/aurafx-128.png" "$PROJ/app/src/main/res/mipmap-$d/ic_launcher.png"
  fi
done
sed -e "s/@VERSION_NAME@/$VERSION/" -e "s/@VERSION_CODE@/$CODE/" "$ROOT/packaging/android/studio/version.properties" > "$PROJ/version.properties"
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/THIRD-PARTY-NOTICES.txt" "$PROJ/"

# The Gradle wrapper (gradlew and its jar), when Gradle is here to write it.
if command -v gradle >/dev/null; then
  (cd "$PROJ" && gradle wrapper --gradle-version 9.6.0 --quiet) || echo "gradle wrapper failed; the zip ships without gradlew" >&2
  rm -rf "$PROJ/.gradle"
fi
(cd "$WORK" && zip -qry "$OUT/auraFX-$VERSION-android-studio.zip" auraFX-android-studio)
echo "Built $OUT/auraFX-$VERSION-android-studio.zip (version $VERSION, code $CODE)"

if [ "${ANDROID_STUDIO_VERIFY:-0}" = "1" ]; then
  (cd "$PROJ" && ./gradlew --quiet bundleRelease && ls -la app/build/outputs/bundle/release/)
  echo "The Android Studio project builds its bundle."
fi
