//! Android entry point. Build with:
//! `cargo ndk -t arm64-v8a -o target/android-libs build -p aurafx-android --release`
//! then package with `scripts/package/android_apk.sh`.

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    if let Some(dir) = app.internal_data_path() {
        // SAFETY: set before any other thread is started.
        unsafe { std::env::set_var("AURAFX_DATA_DIR", &dir) };
        unsafe { std::env::set_var("HOME", &dir) };
    }
    aurafx_app::init_logging();
    let mut options = aurafx_app::native_options();
    options.android_app = Some(app);
    if let Err(e) = aurafx_app::run(options, Vec::new(), None) {
        eprintln!("auraFX exited with an error: {e}");
    }
}
