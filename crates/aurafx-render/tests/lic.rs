//! Surface LIC: streaks follow the vector field on the surface.

use aurafx_data::{Array, ArrayData, DataSet};
use aurafx_pipeline::{DisplayProps, Representation};
use aurafx_render::*;
use std::sync::Arc;

/// A plane with the vector field `f(x, y)` on its points.
fn field_plane(f: impl Fn(f64, f64) -> [f64; 3]) -> DataSet {
    let mut pd = aurafx_filters::sources::plane([-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, 1.0, 0.0], 40, 40);
    let v: Vec<f64> = (0..pd.points.len())
        .flat_map(|i| {
            let p = pd.points.get(i);
            f(p[0], p[1])
        })
        .collect();
    pd.point_data.insert(Array::new("v", 3, ArrayData::F64(v)));
    DataSet::PolyData(pd)
}

/// Mean absolute difference between neighbours along rows and along columns, over the middle
/// of the picture.
fn roughness(img: &[u8], w: u32, h: u32) -> (f64, f64) {
    let g = |x: u32, y: u32| img[(4 * (y * w + x)) as usize] as f64;
    let (mut along_x, mut along_y, mut n) = (0.0, 0.0, 0.0);
    for y in h / 4..3 * h / 4 {
        for x in w / 4..3 * w / 4 {
            along_x += (g(x + 1, y) - g(x, y)).abs();
            along_y += (g(x, y + 1) - g(x, y)).abs();
            n += 1.0;
        }
    }
    (along_x / n, along_y / n)
}

#[test]
fn streaks_follow_the_field() {
    let (w, h) = (240u32, 240u32);
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let mut d = DisplayProps::default();
    d.representation = Representation::SurfaceLic;
    d.lighting = false;
    let mut cam = Camera::default();
    let flat = field_plane(|_, _| [1.0, 0.0, 0.0]);
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &flat.bounds());
    cam.update_clip(&flat.bounds());
    let settings = RenderSettings::default();
    let draw = |ds: &DataSet, d: &DisplayProps| NodeDraw { id: 1, mesh: Arc::new(tessellate::tessellate(ds, d)), display: d.clone(), overlay: false };
    let dump = |name: &str, img: &[u8]| {
        if let Ok(dir) = std::env::var("AURAFX_DUMP") {
            let _ = image::save_buffer(format!("{dir}/{name}.png"), img, w, h, image::ColorType::Rgba8);
        }
    };

    // Horizontal field: smooth along rows, rough across them.
    r.render(&cam, &[draw(&flat, &d)], &settings);
    let (_, _, img) = r.read_pixels().unwrap();
    dump("lic_x", &img);
    let (ax, ay) = roughness(&img, w, h);
    eprintln!("horizontal field: along x {ax:.2}, along y {ay:.2}");
    assert!(ay > 3.0 * ax, "streaks run along x");

    // Vertical field: the other way round.
    let up = field_plane(|_, _| [0.0, 1.0, 0.0]);
    r.render(&cam, &[draw(&up, &d)], &settings);
    let (_, _, img) = r.read_pixels().unwrap();
    let (bx, by) = roughness(&img, w, h);
    eprintln!("vertical field: along x {bx:.2}, along y {by:.2}");
    assert!(bx > 3.0 * by, "streaks run along y");

    // A vortex (for looking at), and a plain surface stays untextured.
    let vortex = field_plane(|x, y| [-y, x, 0.0]);
    r.render(&cam, &[draw(&vortex, &d)], &settings);
    let (_, _, img) = r.read_pixels().unwrap();
    dump("lic_vortex", &img);
    let mut plain = d.clone();
    plain.representation = Representation::Surface;
    r.render(&cam, &[draw(&vortex, &plain)], &settings);
    let (_, _, img) = r.read_pixels().unwrap();
    let (px, py) = roughness(&img, w, h);
    assert!(px < 0.5 && py < 0.5, "no streaks on a plain surface: {px} {py}");
}
