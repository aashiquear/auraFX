# auraFX for iOS: the Xcode project

This folder is a complete Xcode project of the auraFX viewer (no embedded Python). The app
itself is the prebuilt library `lib/libaurafx_ios.a` (arm64 devices; there is no simulator
build), so Xcode only links, signs and packages: no Rust toolchain is needed.

## Sign and run with your Apple developer account

1. Open `auraFX.xcodeproj` in Xcode (15 or newer).
2. Select the **auraFX** target › **Signing & Capabilities**, tick *Automatically manage
   signing* and choose your **Team**. Change the **Bundle Identifier** (`org.aurafx.app`) to
   one registered to your account if that one is taken.
3. Choose your device (or *Any iOS Device (arm64)*) and press **Run**, or **Product › Archive**
   and then **Distribute App** for TestFlight, Ad Hoc or the App Store.

The version is *Marketing Version* and the build number *Current Project Version* in the
target's Build Settings (every upload to App Store Connect needs a higher build number).

From a terminal:

    xcodebuild -project auraFX.xcodeproj -scheme auraFX -configuration Release \
        -archivePath build/auraFX.xcarchive archive \
        DEVELOPMENT_TEAM=YOURTEAMID -allowProvisioningUpdates

## Rebuilding from source

`scripts/package/ios_xcode.sh` in the auraFX repository builds the library and writes this
folder (macOS with Xcode, Rust with the `aarch64-apple-ios` target, and XcodeGen). The project
is described by `project.yml`; `xcodegen generate` writes `auraFX.xcodeproj` again from it.

## Without a Mac

The unsigned `.ipa` itself is built on Linux as well: `scripts/package/ios.sh` uses the iOS SDK
and linker that [xtool](https://xtool.sh) sets up (`xtool setup`), and
`xtool install auraFX-<version>-ios-unsigned.ipa` signs it for your Apple account and puts it
on a connected iPad or iPhone.

## Signing the unsigned .ipa instead

`scripts/package/ios_sign.sh auraFX-<version>-ios-unsigned.ipa "Apple Development: Your Name (ID)" profile.mobileprovision`
signs the release's unsigned `.ipa` with an identity from your keychain and a provisioning
profile for the app's bundle identifier.
