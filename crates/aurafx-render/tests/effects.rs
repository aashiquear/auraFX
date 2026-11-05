//! Custom lights, shadows, ambient occlusion, FXAA and tone mapping.

use aurafx_data::DataSet;
use aurafx_pipeline::DisplayProps;
use aurafx_render::*;
use std::sync::Arc;

fn draw(id: u64, ds: &DataSet, color: [f32; 3]) -> NodeDraw {
    let mut d = DisplayProps::default();
    d.solid_color = color;
    NodeDraw { id, mesh: Arc::new(tessellate::tessellate(ds, &d)), display: d, overlay: false }
}

fn plane(size: f64) -> DataSet {
    DataSet::PolyData(aurafx_filters::sources::plane([-size, -size, 0.0], [size, -size, 0.0], [-size, size, 0.0], 8, 8))
}

fn lum(img: &[u8], i: usize) -> f64 {
    img[4 * i] as f64 + img[4 * i + 1] as f64 + img[4 * i + 2] as f64
}

fn dump(name: &str, img: &[u8], w: u32, h: u32) {
    if let Ok(d) = std::env::var("AURAFX_DUMP") {
        let _ = image::save_buffer(format!("{d}/{name}.png"), img, w, h, image::ColorType::Rgba8);
    }
}

#[test]
fn lights_shadows_occlusion_and_final_pass() {
    let (w, h) = (240u32, 200u32);
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let floor = plane(1.5);
    let ball = DataSet::PolyData(aurafx_filters::sources::sphere([0.0, 0.0, 0.45], 0.4, 48, 24));
    let scene = [draw(1, &floor, [0.85, 0.85, 0.85]), draw(2, &ball, [0.85, 0.85, 0.85])];
    let bounds = floor.bounds().union(&ball.bounds());
    let n = (w * h) as usize;

    // Seen from straight above, lit by one light fixed in the scene, from +x and above.
    let mut top = Camera::default();
    top.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &bounds);
    top.update_clip(&bounds);
    let sun = Light { direction: [0.6, 0.0, 1.0], intensity: 0.8, camera_relative: false, shadows: true, ..Default::default() };
    let base = RenderSettings { lighting: Lighting { lights: vec![sun], ambient: Some(0.2), ..Default::default() }, ..Default::default() };
    r.render(&top, &scene, &base);
    let (_, _, lit) = r.read_pixels().unwrap();
    r.render(&top, &scene, &RenderSettings { lighting: Lighting { shadows: true, ..base.lighting.clone() }, ..base.clone() });
    let (_, _, shadowed) = r.read_pixels().unwrap();
    dump("shadow", &shadowed, w, h);
    let dark: Vec<usize> = (0..n).filter(|&i| lum(&lit, i) - lum(&shadowed, i) > 60.0).collect();
    let left = dark.iter().filter(|&&i| (i as u32 % w) < w / 2).count();
    eprintln!("shadow: {} pixels darker, {} of them on the side away from the light", dark.len(), left);
    assert!(dark.len() > 300, "the ball casts a shadow");
    assert!(left as f64 > 0.9 * dark.len() as f64, "the shadow falls away from the light");

    // A colored light colors the surface.
    let red = Light { color: [1.0, 0.2, 0.2], intensity: 0.9, ..Default::default() };
    r.render(&top, &scene[..1], &RenderSettings { lighting: Lighting { lights: vec![red], ambient: Some(0.05), ..Default::default() }, ..Default::default() });
    let (_, _, img) = r.read_pixels().unwrap();
    let c = &img[4 * (n / 2 + w as usize / 2 - 20)..][..3];
    assert!(c[0] as i32 > c[1] as i32 + 60, "red light: {c:?}");

    // Ambient occlusion darkens the floor around the ball's base and leaves open floor alone.
    let mut side = Camera::default();
    side.set_view([0.0, -1.0, 0.7], [0.0, 0.0, 1.0], &bounds);
    side.update_clip(&bounds);
    r.render(&side, &scene, &RenderSettings::default());
    let (_, _, plain) = r.read_pixels().unwrap();
    r.render(&side, &scene, &RenderSettings { lighting: Lighting { ssao: true, ..Default::default() }, ..Default::default() });
    let (_, _, ao) = r.read_pixels().unwrap();
    dump("ssao", &ao, w, h);
    let depth = r.read_depth().unwrap();
    let darker = |i: usize| lum(&plain, i) - lum(&ao, i);
    let occluded = (0..n).filter(|&i| depth[i] < 1.0 && darker(i) > 30.0).count();
    // Open floor: its ten rows nearest the viewer, far from the ball.
    let last_row = (0..n).filter(|&i| depth[i] < 1.0).map(|i| i / w as usize).max().unwrap();
    let open: Vec<usize> = (w as usize * (last_row - 10)..w as usize * (last_row + 1)).filter(|&i| depth[i] < 1.0).collect();
    let open_dark = open.iter().map(|&i| darker(i)).sum::<f64>() / open.len().max(1) as f64;
    eprintln!("ssao: {occluded} pixels darkened, open floor by {open_dark:.1}");
    assert!(occluded > 100, "creases are darkened");
    assert!(open_dark < 8.0, "open floor stays lit");

    // FXAA only touches edges; Reinhard tone mapping compresses brightness.
    r.render(&side, &scene, &RenderSettings { lighting: Lighting { fxaa: true, ..Default::default() }, ..Default::default() });
    let (_, _, aa) = r.read_pixels().unwrap();
    let changed = (0..n).filter(|&i| (lum(&aa, i) - lum(&plain, i)).abs() > 6.0).count();
    eprintln!("fxaa changed {changed} pixels");
    assert!(changed > 0 && changed < n / 10, "FXAA smooths edges only");
    r.render(&side, &scene, &RenderSettings { lighting: Lighting { tone_mapping: ToneMapping::Reinhard, ..Default::default() }, ..Default::default() });
    let (_, _, tm) = r.read_pixels().unwrap();
    let i = open[open.len() / 2];
    assert!(lum(&tm, i) < lum(&plain, i) - 30.0, "tone mapping compresses: {} vs {}", lum(&tm, i), lum(&plain, i));
}

#[test]
fn transparent_background() {
    let (w, h) = (120u32, 100u32);
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let ball = DataSet::PolyData(aurafx_filters::sources::sphere([0.0, 0.0, 0.0], 0.5, 48, 24));
    let mut cam = Camera::default();
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &ball.bounds());
    cam.update_clip(&ball.bounds());
    let mut d = DisplayProps::default();
    d.solid_color = [0.9, 0.2, 0.1];
    d.lighting = false;
    let draws = [NodeDraw { id: 1, mesh: Arc::new(tessellate::tessellate(&ball, &d)), display: d, overlay: false }];
    // A bright background would tint edges if the colors weren't unpremultiplied.
    let settings = RenderSettings { background: [1.0, 1.0, 1.0], transparent: true, ..Default::default() };
    for fxaa in [false, true] {
        let settings = RenderSettings { lighting: Lighting { fxaa, ..Default::default() }, ..settings.clone() };
        r.render(&cam, &draws, &settings);
        let dir = std::env::temp_dir().join(format!("aurafx_transparent_{fxaa}.png"));
        r.save_png_with(&dir, true).unwrap();
        let img = image::open(&dir).unwrap().to_rgba8();
        let _ = std::fs::remove_file(&dir);
        assert_eq!(img.get_pixel(0, 0)[3], 0, "background is transparent");
        let m = img.get_pixel(w / 2, h / 2).0;
        assert!(
            m[3] == 255 && (m[0] as i32 - 243).abs() <= 1 && (m[1] as i32 - 124).abs() <= 1 && (m[2] as i32 - 89).abs() <= 1,
            "solid color in the middle: {m:?}"
        );
        // Partly covered edge pixels keep the ball's color (no white fringe).
        let edge: Vec<_> = img.pixels().filter(|p| p[3] > 20 && p[3] < 235).collect();
        assert!(!edge.is_empty(), "anti-aliased edge");
        for p in &edge {
            assert!(p[1] < 160 && p[2] < 140, "edge pixel {p:?} is tinted by the background");
        }
    }
}

/// Cells with NaN values are drawn in the NaN color (grey), not the low end of the map.
#[test]
fn nan_cells_are_grey() {
    let (w, h) = (80u32, 60u32);
    let Ok(mut r) = headless(w, h) else { return };
    let mut pd = aurafx_filters::sources::plane([-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, 1.0, 0.0], 2, 1);
    pd.cell_data.insert(aurafx_data::Array::scalars_f64("v", vec![f64::NAN, 10.0]));
    let ds = DataSet::PolyData(pd);
    let mut d = DisplayProps::default();
    d.color_by = Some(aurafx_pipeline::ColorBy { assoc: aurafx_filters::Association::Cells, name: "v".into(), component: None });
    d.lighting = false;
    d.range = Some((0.0, 20.0));
    let mut cam = Camera::default();
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &ds.bounds());
    cam.update_clip(&ds.bounds());
    r.render(&cam, &[NodeDraw { id: 1, mesh: Arc::new(tessellate::tessellate(&ds, &d)), display: d, overlay: false }], &RenderSettings::default());
    let (_, _, img) = r.read_pixels().unwrap();
    let px = |x: u32, y: u32| &img[(4 * (y * w + x)) as usize..][..3];
    let nan = px(w / 2 - 15, h / 2);
    assert!((nan[0] as i32 - nan[2] as i32).abs() < 6 && (nan[0] as i32 - 188).abs() < 8, "NaN cell is grey: {nan:?}");
}
