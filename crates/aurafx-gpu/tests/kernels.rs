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

/// Run `f` without the GPU, then with it; time both.
fn both<T>(f: impl Fn() -> T) -> (T, T, f64, f64) {
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
