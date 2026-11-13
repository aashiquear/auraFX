//! iOS: draw over the whole screen, and keep the system's strips clear ourselves.
//!
//! The windowing library reports the safe area as the window's inner size, and the picture is
//! drawn at that size, while the interface is laid out for (and touches arrive in) the whole
//! screen. The smaller picture is then stretched over the screen: everything appears lower than
//! where a touch on it lands. Here the app's window answers "no safe-area insets" to the windowing
//! library, so the picture has the size of the screen, and the real insets are kept for
//! [`crate::touch`], which reserves the strips inside the interface.

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

/// `UIEdgeInsets`.
#[repr(C)]
#[derive(Clone, Copy)]
struct EdgeInsets {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
}

type Getter = unsafe extern "C" fn(*mut c_void, *mut c_void) -> EdgeInsets;

extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn class_getInstanceMethod(cls: *mut c_void, sel: *mut c_void) -> *mut c_void;
    fn class_getMethodImplementation(cls: *mut c_void, sel: *mut c_void) -> *mut c_void;
    fn method_getTypeEncoding(method: *mut c_void) -> *const c_char;
    fn class_replaceMethod(cls: *mut c_void, sel: *mut c_void, imp: *mut c_void, types: *const c_char) -> *mut c_void;
    fn object_getClassName(obj: *mut c_void) -> *const c_char;
}

static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// The real insets of the app's window (UIKit points): top, bottom, left, right, as f32 bits.
static REAL: [AtomicU32; 4] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

unsafe extern "C" fn safe_area_insets(this: *mut c_void, sel: *mut c_void) -> EdgeInsets {
    let original = ORIGINAL.load(Ordering::Relaxed);
    if original.is_null() {
        return EdgeInsets { top: 0.0, left: 0.0, bottom: 0.0, right: 0.0 };
    }
    let original: Getter = std::mem::transmute(original);
    let real = original(this, sel);
    // Only the app's own window: the system's windows (the keyboard's) keep their answer.
    let name = object_getClassName(this);
    if name.is_null() || CStr::from_ptr(name).to_bytes() != b"WinitUIWindow" {
        return real;
    }
    for (slot, v) in REAL.iter().zip([real.top, real.bottom, real.left, real.right]) {
        slot.store((v.max(0.0) as f32).to_bits(), Ordering::Relaxed);
    }
    EdgeInsets { top: 0.0, left: 0.0, bottom: 0.0, right: 0.0 }
}

/// Call once, before the window is made.
pub fn install() {
    unsafe {
        let cls = objc_getClass(c"UIWindow".as_ptr());
        let sel = sel_registerName(c"safeAreaInsets".as_ptr());
        if cls.is_null() || sel.is_null() {
            return;
        }
        let method = class_getInstanceMethod(cls, sel);
        let original = class_getMethodImplementation(cls, sel);
        if method.is_null() || original.is_null() {
            return;
        }
        ORIGINAL.store(original, Ordering::Relaxed);
        class_replaceMethod(cls, sel, safe_area_insets as *mut c_void, method_getTypeEncoding(method));
    }
}

/// True once [`install`] has taken effect.
pub fn installed() -> bool {
    !ORIGINAL.load(Ordering::Relaxed).is_null()
}

/// The system's insets in UIKit points: `[top, bottom, left, right]`.
pub fn insets() -> [f32; 4] {
    [0, 1, 2, 3].map(|k| f32::from_bits(REAL[k].load(Ordering::Relaxed)))
}
