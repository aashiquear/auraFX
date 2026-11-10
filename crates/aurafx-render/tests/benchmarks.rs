//! Benchmarks for the user guide (docs/userguide/src/benchmarks.md): reading files, common
//! filters on a 128³ volume, and drawing frames. Not run with the tests; run it in release mode
//! and it writes the page:
//!
//!     cargo test --release -p aurafx-render --test benchmarks -- --ignored --nocapture
use aurafx_pipeline::Session;
use aurafx_render::*;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// The best of `n` runs, in milliseconds.
fn best(n: usize, mut f: impl FnMut() -> f64) -> f64 {
    (0..n).map(|_| f()).fold(f64::INFINITY, f64::min)
}

#[test]
#[ignore]
fn benchmarks() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut rows: Vec<(String, String, f64)> = Vec::new();
    // Reading files.
    for (f, what) in [("testdata/can.ex2", "Exodus II, 17.6 MB, the first time step"), ("testdata/disk_out_ref.ex2", "Exodus II, 0.7 MB")] {
        let t = best(3, || {
            let mut s = Session::new();
            let t = Instant::now();
            let id = s.open_file(&root.join(f)).unwrap();
            s.update_blocking();
            assert!(s.nodes[&id].error.is_none());
            ms(t)
        });
        rows.push(("Read".into(), format!("{f} ({what})"), t));
    }
    // Filters on a 129³-point wavelet (2.1 million points).
    let mut s = Session::new();
    let w = s.add_source("wavelet").unwrap();
    s.set_param_json(w, "half_size", &json!(64));
    let t = Instant::now();
    s.update_blocking();
    rows.push(("Source".into(), "Wavelet, 129³ points".into(), ms(t)));
    let cases: Vec<(&str, &str, serde_json::Value)> = vec![
        ("contour", "Contour at one value", json!({"values": [157.0]})),
        ("slice", "Slice through the middle", json!({})),
        ("clip", "Clip by a plane", json!({})),
        ("threshold", "Threshold (range 150 to 200)", json!({"lower": 150.0, "upper": 200.0})),
        ("gradient", "Gradient of the scalar", json!({})),
        ("resample_to_image", "Resample To Image (64³)", json!({"nx": 64, "ny": 64, "nz": 64})),
    ];
    for (id, what, params) in cases {
        let t = best(3, || {
            let n = s.add_filter(id, w).unwrap();
            if let Some(o) = params.as_object() {
                for (k, v) in o {
                    s.set_param_json(n, k, v);
                }
            }
            let t = Instant::now();
            s.update_blocking();
            let e = ms(t);
            assert!(s.nodes[&n].error.is_none(), "{id}: {:?}", s.nodes[&n].error);
            s.remove_node(n);
            e
        });
        rows.push(("Filter".into(), format!("{what}, 129³ wavelet"), t));
    }
    // Drawing: the contour of the wavelet, 1280 x 720, frames after the first.
    let c = s.add_filter("contour", w).unwrap();
    s.set_param_json(c, "values", &json!([157.0]));
    s.update_blocking();
    let out = s.nodes[&c].output.clone().unwrap();
    let display = s.nodes[&c].display.clone();
    let (wd, ht) = (1280u32, 720u32);
    let mut gpu_name = String::from("no GPU adapter");
    if let Ok(mut r) = headless(wd, ht) {
        gpu_name = r.adapter_info.clone();
        let t = Instant::now();
        let mesh = Arc::new(tessellate::tessellate(&out, &display));
        rows.push(("Draw".into(), format!("Prepare the contour for drawing ({} triangles)", mesh.tris.len() / 3), ms(t)));
        let mut cam = Camera::default();
        let b = out.bounds();
        cam.set_view([1.0, -1.0, 0.6], [0.0, 0.0, 1.0], &b);
        cam.update_clip(&b);
        let draws = [NodeDraw { id: 1, mesh, display, overlay: false }];
        r.render(&cam, &draws, &RenderSettings::default());
        let _ = r.read_pixels();
        let n = 30;
        let t = Instant::now();
        for k in 0..n {
            cam.rotate(0.02 * k as f64, 0.0);
            r.render(&cam, &draws, &RenderSettings::default());
        }
        let _ = r.read_pixels();
        let per = ms(t) / n as f64;
        rows.push(("Draw".into(), format!("A frame of the contour at {wd} x {ht}, 4x MSAA ({:.0} frames per second)", 1000.0 / per), per));
    }
    // The page.
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|c| c.lines().find(|l| l.starts_with("model name")).map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string()))
        .unwrap_or_else(|| std::env::consts::ARCH.into());
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let mut page = String::from("# Benchmarks\n\n<!-- Written by crates/aurafx-render/tests/benchmarks.rs; run it again to update. -->\n\nHow long common steps take, measured on one machine with the release build (the best of three runs; filters use all cores). Run them on your own machine with\n\n```sh\ncargo test --release -p aurafx-render --test benchmarks -- --ignored --nocapture\n```\n\nwhich rewrites this page.\n\n");
    page += &format!("Machine: {cpu}, {cores} threads; GPU: {gpu_name}.\n\n| Step | What | Time (ms) |\n|---|---|---|\n");
    for (k, w, t) in &rows {
        page += &format!("| {k} | {w} | {t:.1} |\n");
    }
    for (k, w, t) in &rows {
        eprintln!("{k:8} {t:9.1} ms  {w}");
    }
    std::fs::write(root.join("docs/userguide/src/benchmarks.md"), page).unwrap();
}
