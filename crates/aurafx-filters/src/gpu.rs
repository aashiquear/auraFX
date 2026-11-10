//! Filters that use the GPU when there is one (the `aurafx-gpu` crate installs it at start).
//!
//! The GPU computes in 32-bit, so it never decides a result: it finds which cells a contour or
//! slice can touch, with a margin (a few more cells than needed, never fewer); the CPU then
//! finishes the work on those in 64-bit. The
//! results are the same as without a GPU, value for value; only the time changes. Filters fall
//! back to the CPU when the GPU is off, missing, out of memory or the data is small.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Below this many cells (or values) the CPU is as fast as sending the data to the GPU.
pub const MIN_ITEMS: usize = 1 << 20;

pub trait GpuKernels: Send + Sync {
    /// The device, e.g. "AMD Radeon RX 5500 (Vulkan)".
    fn name(&self) -> String;
    /// Cells of a point grid (`dims` points, i fastest) whose corner values may reach one of the
    /// `ranges` (inclusive): every cell whose values cross a value inside a range is in the list
    /// (in increasing order). None: the GPU could not do it (the caller uses the CPU).
    fn grid_candidates(&self, values: &[f32], dims: [usize; 3], ranges: &[(f32, f32)]) -> Option<Vec<u32>>;
    /// Cells of an image (its extent, origin and spacing) whose corners' signed distances to a
    /// plane may reach [-margin, margin] or change sign.
    fn image_plane_candidates(&self, extent: [i64; 6], origin: [f64; 3], spacing: [f64; 3], plane: [f64; 3], normal: [f64; 3], margin: f64)
        -> Option<Vec<u32>>;
}

static KERNELS: OnceLock<Box<dyn GpuKernels>> = OnceLock::new();
static ON: AtomicBool = AtomicBool::new(true);

/// Install the GPU kernels (once, at start).
pub fn install(k: Box<dyn GpuKernels>) {
    let _ = KERNELS.set(k);
}

/// Use the GPU for filters (default on when there is one).
pub fn set_enabled(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The kernels, when installed and switched on.
pub fn kernels() -> Option<&'static dyn GpuKernels> {
    if enabled() {
        KERNELS.get().map(|k| k.as_ref())
    } else {
        None
    }
}

/// The device the filters use, if any.
pub fn device_name() -> Option<String> {
    KERNELS.get().map(|k| k.name())
}

/// Time Contour and Slice on a wavelet of `(2 half + 1)^3` points with and without
/// the GPU: (what, CPU seconds, GPU seconds). Empty without a GPU.
pub fn benchmark(half: i64) -> Vec<(String, f64, f64)> {
    let Some(_) = KERNELS.get() else { return Vec::new() };
    let was = enabled();
    let src = crate::find("wavelet").unwrap();
    let mut p = src.default_params();
    p.0.insert("half_size".into(), crate::ParamValue::Int(half.clamp(8, 400)));
    let Ok(img) = src.run(None, &p) else { return Vec::new() };
    let time = |on: bool, f: &dyn Fn()| {
        set_enabled(on);
        let t = std::time::Instant::now();
        f();
        t.elapsed().as_secs_f64()
    };
    let mut out = Vec::new();
    let runs: [(&str, Box<dyn Fn()>); 2] = [
        ("Contour", Box::new(|| drop(crate::contour::contour(&img, crate::Association::Points, "RTData", None, &[150.0])))),
        ("Slice", Box::new(|| drop(crate::slice::slice_plane(&img, [0.3, 0.2, 0.1], [0.2, 0.5, 1.0])))),
    ];
    for (name, f) in runs.iter() {
        // Once each to warm up, then timed.
        time(false, f.as_ref());
        let cpu = time(false, f.as_ref());
        time(true, f.as_ref());
        let gpu = time(true, f.as_ref());
        out.push((name.to_string(), cpu, gpu));
    }
    set_enabled(was);
    out
}

/// A value array (component `comp`, or the only one) as 32-bit numbers.
pub fn as_f32(arr: &aurafx_data::Array, comp: Option<usize>) -> Option<std::borrow::Cow<'_, [f32]>> {
    if arr.ncomp == 1 {
        if let Some(s) = arr.as_slice::<f32>() {
            return Some(std::borrow::Cow::Borrowed(s));
        }
    }
    let c = match (arr.ncomp, comp) {
        (1, _) => 0,
        (_, Some(c)) if c < arr.ncomp => c,
        _ => return None,
    };
    Some(std::borrow::Cow::Owned((0..arr.len()).map(|i| arr.get(i, c) as f32).collect()))
}
