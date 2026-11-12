//! With the GPU, Contour and Slice give exactly the results of the CPU (the GPU only narrows
//! down the cells the CPU then finishes in 64-bit). Skipped without a GPU.
use aurafx_data::{DataSet, PolyData};
use aurafx_filters::{Association, ParamValue};

fn wavelet(half: i64) -> DataSet {
    let src = aurafx_filters::find("wavelet").unwrap();
    let mut p = src.default_params();
    p.0.insert("half_size".into(), ParamValue::Int(half));
    src.run(None, &p).unwrap()
}

fn same(a: &PolyData, b: &PolyData, what: &str) {
    assert_eq!(a.polys.connectivity, b.polys.connectivity, "{what}: cells");
    assert_eq!(a.points.len(), b.points.len(), "{what}: points");
    assert!((0..a.points.len()).all(|i| a.points.get(i) == b.points.get(i)), "{what}: coordinates");
    for arr in b.point_data.iter() {
        assert_eq!(a.point_data.get(&arr.name).unwrap().data.to_f64_vec(), arr.data.to_f64_vec(), "{what}: {}", arr.name);
    }
}

/// Run `f` without the GPU, then with it; time both. (One at a time: the switch is shared.)
fn both<T>(f: impl Fn() -> T) -> (T, T, f64, f64) {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    aurafx_filters::gpu::set_enabled(false);
    let t = std::time::Instant::now();
    let cpu = f();
    let tc = t.elapsed().as_secs_f64();
    aurafx_filters::gpu::set_enabled(true);
    let t = std::time::Instant::now();
    let gpu = f();
    (cpu, gpu, tc, t.elapsed().as_secs_f64())
}

#[test]
fn gpu_results_equal_the_cpu() {
    let Some(name) = aurafx_gpu::install() else {
        eprintln!("no GPU: skipped");
        return;
    };
    let img = wavelet(60); // 121^3 points, 1.7 million cells
    assert!(img.n_cells() >= aurafx_filters::gpu::MIN_ITEMS);
    for vals in [vec![150.0], vec![90.0, 160.0, 230.0]] {
        let (c, g, tc, tg) = both(|| aurafx_filters::contour::contour(&img, Association::Points, "RTData", None, &vals).unwrap());
        assert!(c.n_cells() > 1000);
        same(&g, &c, &format!("contour {vals:?}"));
        eprintln!("{name}: contour {vals:?} CPU {tc:.3} s, GPU {tg:.3} s");
    }
    for (o, n) in [([0.3, -1.2, 0.7], [0.2, 0.5, 1.0]), ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), ([60.0, 0.0, 0.0], [1.0, 0.0, 0.0])] {
        let (c, g, tc, tg) = both(|| aurafx_filters::slice::slice_plane(&img, o, n).unwrap());
        same(&g, &c, &format!("slice {o:?} {n:?}"));
        eprintln!("{name}: slice CPU {tc:.3} s, GPU {tg:.3} s");
    }
}

/// 64-bit values far from zero (32-bit numbers cannot tell neighbouring values apart there),
/// and an image that does not start at the origin with unequal spacing: still exactly the CPU.
#[test]
fn far_values_and_placed_images_equal_the_cpu() {
    let Some(name) = aurafx_gpu::install() else {
        eprintln!("no GPU: skipped");
        return;
    };
    let DataSet::ImageData(mut img) = wavelet(60) else { panic!("an image") };
    assert!(img.n_cells() >= aurafx_filters::gpu::MIN_ITEMS);
    let rt = img.point_data.get("RTData").unwrap();
    let far: Vec<f64> = (0..rt.len()).map(|i| rt.get(i, 0) + 1e6).collect();
    img.point_data.insert(aurafx_data::Array::scalars_f64("Far", far));
    img.origin = [1000.5, -200.25, 3.0];
    img.spacing = [0.1, 0.25, 0.5];
    let img = DataSet::ImageData(img);
    for vals in [vec![1e6 + 150.0], vec![1e6 + 90.0, 1e6 + 160.03, 1e6 + 230.0]] {
        let (c, g, tc, tg) = both(|| aurafx_filters::contour::contour(&img, Association::Points, "Far", None, &vals).unwrap());
        assert!(c.n_cells() > 1000);
        same(&g, &c, &format!("contour of 64-bit values {vals:?}"));
        eprintln!("{name}: contour {vals:?} CPU {tc:.3} s, GPU {tg:.3} s");
    }
    let b = img.bounds();
    let mid: [f64; 3] = std::array::from_fn(|a| 0.5 * (b.min[a] + b.max[a]));
    for (o, n) in [
        (mid, [0.2, 0.5, 1.0]),
        (mid, [1.0, 0.0, 0.0]),
        ([b.max[0], mid[1], mid[2]], [1.0, 0.0, 0.0]),
        ([mid[0] + 0.033, mid[1], b.min[2] + 0.5], [0.0, 0.1, 1.0]),
    ] {
        let (c, g, tc, tg) = both(|| aurafx_filters::slice::slice_plane(&img, o, n).unwrap());
        assert!(c.n_cells() > 0, "slice {o:?} {n:?} cuts the image");
        same(&g, &c, &format!("slice of a placed image {o:?} {n:?}"));
        eprintln!("{name}: slice CPU {tc:.3} s, GPU {tg:.3} s");
    }
}

/// Timings on a bigger grid (AFX_GPU_HALF=200 gives 401^3 points; run with --ignored).
#[test]
#[ignore]
fn timings() {
    let half: i64 = std::env::var("AFX_GPU_HALF").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let Some(name) = aurafx_gpu::install() else { return };
    for (n, c, g) in aurafx_filters::gpu::benchmark(half) {
        eprintln!("{name} {half}: {n} CPU {c:.3} s, GPU {g:.3} s");
    }
}
