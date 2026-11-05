//! The path tracer: same picture and depth as the rasterizer where they should agree, shadows
//! and ambient occlusion where they shouldn't, and noise that goes down with more passes.

use aurafx_data::DataSet;
use aurafx_pipeline::DisplayProps;
use aurafx_render::raytrace::{Scene, Tracer, View};
use aurafx_render::*;
use std::sync::Arc;

fn draw(id: u64, ds: &DataSet, color: [f32; 3]) -> NodeDraw {
    let mut d = DisplayProps::default();
    d.solid_color = color;
    NodeDraw { id, mesh: Arc::new(tessellate::tessellate(ds, &d)), display: d, overlay: false }
}

fn lum(img: &[u8], i: usize) -> f64 {
    img[4 * i] as f64 + img[4 * i + 1] as f64 + img[4 * i + 2] as f64
}

#[test]
fn path_tracer() {
    let (w, h) = (160u32, 120u32);
    let floor = DataSet::PolyData(aurafx_filters::sources::plane([-1.5, -1.5, 0.0], [1.5, -1.5, 0.0], [-1.5, 1.5, 0.0], 4, 4));
    let ball = DataSet::PolyData(aurafx_filters::sources::sphere([0.0, 0.0, 0.45], 0.4, 48, 24));
    let draws = [draw(1, &floor, [0.8, 0.8, 0.8]), draw(2, &ball, [0.8, 0.3, 0.2])];
    let bounds = floor.bounds().union(&ball.bounds());
    let mut cam = Camera::default();
    cam.set_view([0.3, -1.0, 0.8], [0.0, 0.0, 1.0], &bounds);
    cam.update_clip(&bounds);
    let sun = Light { direction: [0.4, 0.2, 1.0], intensity: 0.8, camera_relative: false, ..Default::default() };
    let lighting = Lighting { lights: vec![sun], ambient: Some(0.2), ..Default::default() };
    let scene = Arc::new(Scene::build(&draws));
    assert_eq!(scene.triangles(), (draws[0].mesh.tris.len() + draws[1].mesh.tris.len()) / 3);
    let view = View { camera: cam.clone(), lighting: lighting.clone(), width: w, height: h, background: [0.0; 3], bounces: 2, environment: None };
    let t0 = std::time::Instant::now();
    let mut tracer = Tracer::new(scene.clone(), view.clone());
    tracer.pass();
    let one = tracer.image();
    for _ in 1..32 {
        tracer.pass();
    }
    let many = tracer.image();
    eprintln!("32 passes of {w}x{h} over {} triangles: {:.0} ms", scene.triangles(), t0.elapsed().as_secs_f64() * 1e3);
    if let Ok(d) = std::env::var("AURAFX_DUMP") {
        let _ = image::save_buffer(format!("{d}/rt.png"), &many.color, w, h, image::ColorType::Rgba8);
    }

    // Depth agrees with the rasterizer's.
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let settings = RenderSettings { lighting: lighting.clone(), ..Default::default() };
    r.render(&cam, &draws, &settings);
    let (_, _, raster) = r.read_pixels().unwrap();
    let depth = r.read_depth().unwrap();
    let n = (w * h) as usize;
    let both: Vec<usize> = (0..n).filter(|&i| depth[i] < 1.0 && many.depth[i] < 1.0).collect();
    let coverage_diff = (0..n).filter(|&i| (depth[i] < 1.0) != (many.depth[i] < 1.0)).count();
    assert!(coverage_diff * 25 < both.len(), "same silhouettes: {coverage_diff} of {} differ", both.len());
    // Both push surfaces back by their slope (so edges drawn on them stay in front); the raster
    // depth is read at a multisample position, a fraction of a pixel's slope off the center.
    // Pixels on the ball's outline may see the ball in one and the floor in the other.
    let off = both.iter().filter(|&&i| (depth[i] - many.depth[i]).abs() > 1.5e-3).count();
    eprintln!("depth: {off} of {} pixels differ by more than 1.5e-3", both.len());
    assert!(off * 25 < both.len());

    // Open, sunlit floor is about as bright as the rasterizer draws it (the sky adds a little
    // bounce light); the floor under the ball is in shadow.
    let floor_px: Vec<usize> = both.iter().copied().filter(|&i| raster[4 * i] as i32 - raster[4 * i + 2] as i32 <= 10).collect();
    let ratio = floor_px.iter().map(|&i| lum(&many.color, i) / lum(&raster, i).max(1.0)).sum::<f64>() / floor_px.len() as f64;
    let shadowed = floor_px.iter().filter(|&&i| lum(&many.color, i) < 0.6 * lum(&raster, i)).count();
    eprintln!("floor: traced / raster brightness {ratio:.2}, {shadowed} pixels in shadow");
    assert!((0.8..1.35).contains(&ratio), "brightness {ratio}");
    assert!(shadowed > 60, "the ball casts a shadow");

    // More passes, less noise: distance from a 128-pass reference.
    for _ in 32..128 {
        tracer.pass();
    }
    let reference = tracer.image();
    let err = |img: &[u8]| floor_px.iter().map(|&i| (lum(img, i) - lum(&reference.color, i)).abs()).sum::<f64>() / floor_px.len() as f64;
    let (e1, e32) = (err(&one.color), err(&many.color));
    eprintln!("noise against 128 passes: 1 pass {e1:.1}, 32 passes {e32:.1}");
    assert!(e32 < e1 / 3.0);

    // The same scene gives the same picture (deterministic sampling).
    let mut again = Tracer::new(scene, view);
    again.pass();
    assert_eq!(again.image().color, one.color);
}
