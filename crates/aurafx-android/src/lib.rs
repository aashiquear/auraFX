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
    // The strips the system keeps (status bar, camera cut-out, navigation or gesture bar, and the
    // on-screen keyboard while it is up): the app leaves them free.
    {
        let app = app.clone();
        aurafx_app::touch::set_insets_source(move || insets::read(&app));
    }
    let mut options = aurafx_app::native_options();
    options.android_app = Some(app);
    if let Err(e) = aurafx_app::run(options, Vec::new(), None) {
        eprintln!("auraFX exited with an error: {e}");
    }
}

/// The insets of the app's window, asked of the system through JNI: the windowing library does
/// not report them, and with the screen drawn edge to edge (Android 15 and later) the activity's
/// content rectangle is the whole window.
#[cfg(target_os = "android")]
mod insets {
    use jni::objects::JObject;
    use jni::strings::JNIStr;
    use jni::{jni_sig, jni_str, Env, JValue, JavaVM};
    use winit::platform::android::activity::AndroidApp;

    fn int(env: &mut Env, obj: &JObject, name: &JNIStr) -> jni::errors::Result<i32> {
        env.call_method(obj, name, jni_sig!("()I"), &[])?.i()
    }

    /// `[top, bottom, left, right]` in pixels; `None` while the window has no insets yet or when
    /// the system would not say.
    pub fn read(app: &AndroidApp) -> Option<[f32; 4]> {
        // SAFETY: the pointers are the process's Java VM and the activity this app runs in, both
        // valid while `android_main` runs; the activity reference is only borrowed.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        let activity = app.activity_as_ptr();
        let r = vm.attach_current_thread(|env| -> jni::errors::Result<Option<[f32; 4]>> {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            let window = env.call_method(&activity, jni_str!("getWindow"), jni_sig!("()Landroid/view/Window;"), &[])?.l()?;
            if window.is_null() {
                return Ok(None);
            }
            let decor = env.call_method(&window, jni_str!("getDecorView"), jni_sig!("()Landroid/view/View;"), &[])?.l()?;
            if decor.is_null() {
                return Ok(None);
            }
            let insets = env.call_method(&decor, jni_str!("getRootWindowInsets"), jni_sig!("()Landroid/view/WindowInsets;"), &[])?.l()?;
            if insets.is_null() {
                return Ok(None);
            }
            // The system bars (and the keyboard, which the system counts here for a window that
            // asks to be resized for it, as this one does in its manifest).
            let mut out = [
                int(env, &insets, jni_str!("getSystemWindowInsetTop"))?,
                int(env, &insets, jni_str!("getSystemWindowInsetBottom"))?,
                int(env, &insets, jni_str!("getSystemWindowInsetLeft"))?,
                int(env, &insets, jni_str!("getSystemWindowInsetRight"))?,
            ];
            // A camera cut-out (Android 9 and later; older systems do not have the method).
            match env.call_method(&insets, jni_str!("getDisplayCutout"), jni_sig!("()Landroid/view/DisplayCutout;"), &[]).and_then(|v| v.l()) {
                Ok(cutout) if !cutout.is_null() => {
                    let names = [jni_str!("getSafeInsetTop"), jni_str!("getSafeInsetBottom"), jni_str!("getSafeInsetLeft"), jni_str!("getSafeInsetRight")];
                    for (k, name) in names.into_iter().enumerate() {
                        out[k] = out[k].max(int(env, &cutout, name)?);
                    }
                }
                Ok(_) => {}
                Err(_) => env.exception_clear(),
            }
            // The part of the window that is in view. A window that fills the screen is not
            // always resized for the keyboard and may not have it in its insets: what the
            // keyboard covers is then only seen here.
            let covered = (|| -> jni::errors::Result<i32> {
                let rect = env.new_object(jni_str!("android/graphics/Rect"), jni_sig!("()V"), &[])?;
                env.call_method(&decor, jni_str!("getWindowVisibleDisplayFrame"), jni_sig!("(Landroid/graphics/Rect;)V"), &[JValue::Object(&rect)])?;
                let h = int(env, &decor, jni_str!("getHeight"))?;
                let bottom = env.get_field(&rect, jni_str!("bottom"), jni_sig!("I"))?.i()?;
                let top = env.get_field(&rect, jni_str!("top"), jni_sig!("I"))?.i()?;
                // The frame is in screen coordinates: only trusted for a window that starts at
                // the top of the screen, and never for more than three quarters of its height.
                let below = h - bottom;
                Ok(if h > 0 && bottom > top && below > 0 && below < h / 4 * 3 { below } else { 0 })
            })();
            match covered {
                Ok(b) => out[1] = out[1].max(b),
                Err(_) => env.exception_clear(),
            }
            Ok(Some(out.map(|v| v.max(0) as f32)))
        });
        match r {
            Ok(v) => v,
            Err(e) => {
                eprintln!("auraFX: the system's insets could not be read: {e}");
                None
            }
        }
    }
}
