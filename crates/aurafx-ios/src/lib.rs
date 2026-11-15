//! iOS entry point for an Xcode project: its `main` calls `aurafx_ios_main`, which starts the app
//! on the main thread and does not return. Built by `scripts/package/ios_xcode.sh`.

#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn aurafx_ios_main() {
    if let Err(e) = aurafx_app::main_desktop() {
        eprintln!("auraFX exited with an error: {e}");
    }
}
