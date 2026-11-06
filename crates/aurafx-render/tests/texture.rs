//! Display texture: an image shown on a surface through its texture coordinates.

use aurafx_data::DataSet;
use aurafx_filters::{find, Params};
use aurafx_pipeline::DisplayProps;
use aurafx_render::*;
use std::sync::Arc;

#[test]
fn image_follows_texture_coordinates() {
    let (w, h) = (200u32, 200u32);
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    // An 8×8 image in quarters: red top-left, green top-right, blue bottom-left, white
    // bottom-right.
    let dir = std::env::temp_dir().join(format!("aurafx-texture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("quad.png");
    let quarter = |x: u32, y: u32| match (x < 4, y < 4) {
        (true, true) => [255, 0, 0, 255],
        (false, true) => [0, 255, 0, 255],
        (true, false) => [0, 0, 255, 255],
        (false, false) => [255, 255, 255, 255],
    };
    let px: Vec<u8> = (0..64).flat_map(|i| quarter(i % 8, i / 8)).collect();
    image::save_buffer(&png, &px, 8, 8, image::ColorType::Rgba8).unwrap();

    // A plane in z = 0, mapped by Texture Map to Plane (s along x, t along y).
    let plane = DataSet::PolyData(aurafx_filters::sources::plane([-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, 1.0, 0.0], 8, 8));
    let def = find("texture_map_to_plane").unwrap();
    let mut p: Params = def.default_params();
    p.set_json("automatic", &serde_json::json!(false));
    p.set_json("origin", &serde_json::json!([-1, -1, 0]));
    p.set_json("point1", &serde_json::json!([1, -1, 0]));
    p.set_json("point2", &serde_json::json!([-1, 1, 0]));
    let mapped = def.run(Some(&plane), &p).unwrap();

    let mut d = DisplayProps::default();
    d.lighting = false;
    d.texture = Some(png.to_string_lossy().into_owned());
    let mut cam = Camera::default();
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &mapped.bounds());
    cam.update_clip(&mapped.bounds());
    let mesh = Arc::new(tessellate::tessellate(&mapped, &d));
    assert!(mesh.textured && !mesh.has_scalars);
    r.render(&cam, &[NodeDraw { id: 1, mesh, display: d.clone(), overlay: false }], &RenderSettings::default());
    let (_, _, img) = r.read_pixels().unwrap();
    if let Ok(out) = std::env::var("AURAFX_DUMP") {
        let _ = image::save_buffer(format!("{out}/texture.png"), &img, w, h, image::ColorType::Rgba8);
    }
    let at = |x: u32, y: u32| {
        let i = (4 * (y * w + x)) as usize;
        [img[i], img[i + 1], img[i + 2]]
    };
    // Screen y grows downwards; t = 1 (the image's top row) is at the top.
    let (q1, q3) = (w / 2 - 40, w / 2 + 40);
    let near = |c: [u8; 3], want: [u8; 3]| c.iter().zip(want).all(|(a, b)| (*a as i32 - b as i32).abs() < 40);
    assert!(near(at(q1, q1), [255, 0, 0]), "top left {:?}", at(q1, q1));
    assert!(near(at(q3, q1), [0, 255, 0]), "top right {:?}", at(q3, q1));
    assert!(near(at(q1, q3), [0, 0, 255]), "bottom left {:?}", at(q1, q3));
    assert!(near(at(q3, q3), [255, 255, 255]), "bottom right {:?}", at(q3, q3));

    // Without texture coordinates the image is not used: the solid color shows.
    let plain = DataSet::PolyData(aurafx_filters::sources::cube([0.0; 3], [2.0, 2.0, 0.1]));
    let mesh = Arc::new(tessellate::tessellate(&plain, &d));
    assert!(!mesh.textured);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Surface With Edges draws the polygon edges in the edge color (not from the color map), and
/// a new mesh is uploaded even when it reuses a dropped mesh's memory.
#[test]
fn edges_use_the_edge_color() {
    let (w, h) = (160u32, 160u32);
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let plane = DataSet::PolyData(aurafx_filters::sources::plane([-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, 1.0, 0.0], 4, 4));
    let mut d = DisplayProps::default();
    d.lighting = false;
    d.representation = aurafx_pipeline::Representation::SurfaceWithEdges;
    d.solid_color = [1.0, 1.0, 1.0];
    d.edge_color = [1.0, 0.0, 0.0];
    let mut cam = Camera::default();
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &plane.bounds());
    cam.update_clip(&plane.bounds());
    let reds = |r: &mut Renderer, d: &DisplayProps| {
        r.render(&cam, &[NodeDraw { id: 1, mesh: Arc::new(tessellate::tessellate(&plane, d)), display: d.clone(), overlay: false }], &RenderSettings::default());
        let (_, _, img) = r.read_pixels().unwrap();
        img.chunks(4).filter(|p| p[0] as i32 - p[1] as i32 > 60 && (p[1] as i32 - p[2] as i32).abs() < 30).count()
    };
    let n = reds(&mut r, &d);
    assert!(n > 200, "red edges drawn ({n} pixels)");
    d.representation = aurafx_pipeline::Representation::Surface;
    assert_eq!(reds(&mut r, &d), 0, "no edges once the representation changes");
}

/// The Molecule representation: spheres and cylinders colored by element (Jmol colors).
#[test]
fn molecules_in_element_colors() {
    let (w, h) = (240u32, 240u32);
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/molecules/test.cml");
    let m = match aurafx_io::read_file(&path).unwrap() {
        aurafx_io::Loaded::Static(d) => (*d).clone(),
        _ => panic!(),
    };
    let mut d = DisplayProps::default();
    d.representation = aurafx_pipeline::Representation::Molecule;
    d.color_by = Some(aurafx_pipeline::ColorBy { assoc: aurafx_filters::Association::Points, name: "atomic_number".into(), component: None });
    d.colormap = aurafx_pipeline::colormap::ELEMENTS.into();
    d.lighting = false;
    let mesh = Arc::new(tessellate::tessellate(&m, &d));
    assert_eq!(mesh.scalar_range, Some((-0.5, 127.5)));
    assert!(mesh.tris.len() > 20 * 300, "{} vertices", mesh.tris.len());
    let mut cam = Camera::default();
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &m.bounds());
    cam.update_clip(&m.bounds());
    r.render(&cam, &[NodeDraw { id: 1, mesh, display: d.clone(), overlay: false }], &RenderSettings::default());
    let (_, _, img) = r.read_pixels().unwrap();
    let count = |want: [u8; 3]| img.chunks(4).filter(|p| (0..3).all(|k| (p[k] as i32 - want[k] as i32).abs() < 12)).count();
    // Jmol colors: chlorine (0.12, 0.94, 0.12), oxygen (1, 0.05, 0.05), nitrogen (0.05, 0.05, 1).
    let srgb = |c: [f32; 3]| c.map(|v| (v * 255.0) as u8);
    for (name, c) in [("Cl", [0.12, 0.94, 0.12]), ("O", [1.0, 0.05, 0.05]), ("N", [0.05, 0.05, 1.0])] {
        let n = count(srgb(c));
        assert!(n > 20, "{name}: {n} pixels in its color");
    }
}
