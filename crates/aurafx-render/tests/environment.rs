//! Environment lighting and materials, drawn and path-traced.

use aurafx_data::DataSet;
use aurafx_pipeline::DisplayProps;
use aurafx_render::raytrace::{Scene, Tracer, View};
use aurafx_render::*;
use std::sync::Arc;

fn ball(display: DisplayProps) -> NodeDraw {
    let s = DataSet::PolyData(aurafx_filters::sources::sphere([0.0, 0.0, 0.0], 1.0, 96, 48));
    NodeDraw { id: 1, mesh: Arc::new(tessellate::tessellate(&s, &display)), display, overlay: false }
}

fn px(img: &[u8], w: u32, x: u32, y: u32) -> [i32; 3] {
    let i = (4 * (y * w + x)) as usize;
    [img[i] as i32, img[i + 1] as i32, img[i + 2] as i32]
}

#[test]
fn environment_and_materials() {
    let (w, h) = (160u32, 160u32);
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let mut cam = Camera::default();
    let b = aurafx_data::Bounds { min: [-1.0; 3], max: [1.0; 3] };
    cam.set_view([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], &b);
    cam.update_clip(&b);
    // No lights, a white room: a matte surface shows its own color everywhere.
    let dark = Light { intensity: 0.0, ..Default::default() };
    let white = Lighting { lights: vec![dark], environment: "white".into(), ..Default::default() };
    let mut matte = DisplayProps::default();
    matte.solid_color = [0.5, 0.5, 0.5];
    matte.specular = 0.0;
    matte.roughness = 1.0;
    let draws = [ball(matte.clone())];
    let settings = RenderSettings { lighting: white.clone(), ..Default::default() };
    r.render(&cam, &draws, &settings);
    let (_, _, img) = r.read_pixels().unwrap();
    // 0.5 linear is 188 in sRGB.
    for (x, y) in [(w / 2, h / 2), (w / 2, h / 4), (w / 3, h / 2)] {
        let c = px(&img, w, x, y);
        assert!((c[0] - 188).abs() <= 6, "white room: {c:?} at {x},{y}");
    }
    assert!(r.environment().is_some_and(|e| e.name == "white"));

    // The path tracer agrees.
    let view = View { camera: cam.clone(), lighting: white.clone(), width: w, height: h, background: [0.0; 3], bounces: 2, environment: r.environment() };
    let mut t = Tracer::new(Arc::new(Scene::build(&draws)), view);
    for _ in 0..24 {
        t.pass();
    }
    let traced = t.image();
    let c = px(&traced.color, w, w / 2, h / 2);
    assert!((c[0] - 188).abs() <= 10, "traced white room: {c:?}");

    // Under the sky, a shiny metal ball mirrors the blue sky on its upper half and the ground
    // below; a matte ball doesn't.
    let sky = Lighting { lights: vec![dark], environment: "sky".into(), ..Default::default() };
    let settings = RenderSettings { lighting: sky.clone(), ..Default::default() };
    let mut metal = DisplayProps::default();
    metal.solid_color = [0.9, 0.9, 0.9];
    metal.metallic = 1.0;
    metal.roughness = 0.1;
    r.render(&cam, &[ball(metal)], &settings);
    let (_, _, shiny) = r.read_pixels().unwrap();
    if let Ok(d) = std::env::var("AURAFX_DUMP") {
        let _ = image::save_buffer(format!("{d}/metal_sky.png"), &shiny, w, h, image::ColorType::Rgba8);
    }
    let up = px(&shiny, w, w / 2, h / 2 - 25);
    let down = px(&shiny, w, w / 2, h / 2 + 25);
    eprintln!("metal ball: upper {up:?}, lower {down:?}");
    assert!(up[2] > up[0] + 20, "sky reflected above: {up:?}");
    assert!(down[2] < up[2] - 30, "ground reflected below: {down:?}");

    // The sky as the background: blue above the horizon.
    let settings = RenderSettings { lighting: Lighting { show_environment: true, ..sky }, ..Default::default() };
    r.render(&cam, &[ball(matte)], &settings);
    let (_, _, bg) = r.read_pixels().unwrap();
    let corner = px(&bg, w, 3, 3);
    assert!(corner[2] > corner[0] + 30, "sky behind: {corner:?}");

    // Unknown environments are ignored (with a warning), not fatal.
    let settings = RenderSettings { lighting: Lighting { environment: "/no/such/map.hdr".into(), ..Default::default() }, ..Default::default() };
    r.render(&cam, &draws, &settings);
    assert!(r.environment().is_none(), "a missing map means no environment");
}
