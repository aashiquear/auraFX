# auraFX for Android: the Android Studio project

This folder is a complete Android Studio (Gradle) project of the auraFX viewer (no embedded
Python). The app itself is the prebuilt native library `app/src/main/jniLibs/<abi>/libaurafx.so`
(arm64-v8a and x86_64), so Gradle only packages and signs: no Rust toolchain or NDK is needed.

## A signed app bundle for the Play Console

1. Make an upload key once (keep it and its passwords safe):

       keytool -genkeypair -v -keystore upload.keystore -alias upload -keyalg RSA -keysize 2048 -validity 10000

2. Copy `keystore.properties.example` to `keystore.properties` and fill it in.
3. Set the version in `version.properties`: `versionName` is what users see; `versionCode`
   must be higher than the last upload's (it starts as major*10000 + minor*100 + patch).
4. Build:

       ./gradlew bundleRelease        # app/build/outputs/bundle/release/app-release.aab
       ./gradlew assembleRelease      # app/build/outputs/apk/release/app-release.apk

   or in Android Studio: open this folder, then **Build › Generate Signed App Bundle / APK**
   (it asks for the key itself, so `keystore.properties` is not needed that way).

Upload `app-release.aab` in the Play Console (with Play App Signing the key above is your
upload key). `./gradlew bundleRelease -PversionCode=10001 -PversionName=0.1.0` sets the
version without editing the file. Change `applicationId` in `app/build.gradle` if
`org.aurafx.app` is not yours in the Play Console.

If `gradlew` is missing (a copy of the template made without Gradle installed), open the
folder in Android Studio, which sets Gradle up from `gradle/wrapper/gradle-wrapper.properties`,
or run `gradle wrapper` once.

## Rebuilding from source

`scripts/package/android_studio.sh` in the auraFX repository builds the native libraries
(Rust with the Android targets, cargo-ndk, the NDK) and writes this folder.
