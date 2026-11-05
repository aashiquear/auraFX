//! Image layers: a picture with depth drawn into the scene occludes and is occluded like the
//! geometry it was rendered from.

use aurafx_data::DataSet;
use aurafx_pipeline::DisplayProps;
use aurafx_render::*;
use std::sync::Arc;

#[test]
fn image_layers_composite_with_depth() {
    let (w, h) = (320u32, 240u32);
    let Ok(mut a) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let mut b = headless(w, h).unwrap();
    let sphere = |c: [f64; 3], r: f64| DataSet::PolyData(aurafx_filters::sources::sphere(c, r, 48, 24));
    let draw = |id: u64, ds: &DataSet, color: [f32; 3]| {
        let mut d = DisplayProps::default();
        d.solid_color = color;
        NodeDraw { id, mesh: Arc::new(tessellate::tessellate(ds, &d)), display: d, overlay: false }
    };
    let big = sphere([0.0, 0.0, 0.0], 1.0);
    let small = sphere([0.0, 0.0, 0.9], 0.4); // sticks out of the big one towards the camera
    let mut cam = Camera::default();
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &big.bounds());
    cam.update_clip(&big.bounds().union(&small.bounds()));
    let settings = RenderSettings::default();

    // Reference: both spheres as geometry.
    a.render(&cam, &[draw(1, &big, [0.9, 0.2, 0.2]), draw(2, &small, [0.2, 0.9, 0.2])], &settings);
    let (_, _, both) = a.read_pixels().unwrap();
    // The big sphere alone, as an image with depth.
    a.render(&cam, &[draw(1, &big, [0.9, 0.2, 0.2])], &settings);
    let (_, _, color) = a.read_pixels().unwrap();
    let depth = a.read_depth().unwrap();
    assert_eq!(depth.len(), (w * h) as usize);
    assert!(depth.iter().any(|&d| d < 1.0) && depth.iter().any(|&d| d == 1.0));
    let layer = Arc::new(ImageLayer { width: w, height: h, color, depth });
    // The image plus the small sphere as geometry looks like the reference.
    b.set_layers(&[layer]);
    b.render(&cam, &[draw(2, &small, [0.2, 0.9, 0.2])], &settings);
    let (_, _, comp) = b.read_pixels().unwrap();
    let differ = both.chunks(4).zip(comp.chunks(4)).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| (*p as i32 - *q as i32).abs() > 3)).count();
    let total = (w * h) as usize;
    eprintln!("{differ} of {total} pixels differ");
    // Only anti-aliased silhouette pixels may differ.
    assert!(differ < total / 100, "{differ} pixels differ");
    // The small sphere is in front at the centre, the big one around it.
    let px = |img: &[u8], x: u32, y: u32| img[(4 * (y * w + x)) as usize..][..3].to_vec();
    let centre = px(&comp, w / 2, h / 2);
    assert!(centre[1] > centre[0], "green sphere in front: {centre:?}");
    let side = px(&comp, w / 2 + 40, h / 2);
    assert!(side[0] > side[1], "red sphere around it: {side:?}");
}
