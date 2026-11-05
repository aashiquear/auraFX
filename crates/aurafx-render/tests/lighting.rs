//! Transparency that does not depend on drawing order, Eye-Dome Lighting, opaque points that
//! hide behind surfaces, and the light kit.

use aurafx_data::DataSet;
use aurafx_pipeline::DisplayProps;
use aurafx_render::*;
use std::sync::Arc;

fn draw(id: u64, ds: &DataSet, color: [f32; 3], opacity: f32) -> NodeDraw {
    let mut d = DisplayProps::default();
    d.solid_color = color;
    d.opacity = opacity;
    NodeDraw { id, mesh: Arc::new(tessellate::tessellate(ds, &d)), display: d, overlay: false }
}

fn plane(z: f64, size: f64) -> DataSet {
    DataSet::PolyData(aurafx_filters::sources::plane([-size, -size, z], [size, -size, z], [-size, size, z], 1, 1))
}

#[test]
fn transparency_lighting_and_points() {
    let (w, h) = (200u32, 160u32);
    let Ok(mut r) = headless(w, h) else {
        eprintln!("no GPU adapter");
        return;
    };
    let a = plane(0.0, 1.0);
    let b = plane(0.5, 0.6);
    let mut cam = Camera::default();
    let bounds = a.bounds().union(&b.bounds());
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &bounds);
    cam.update_clip(&bounds);
    let settings = RenderSettings::default();
    let px = |img: &[u8], x: u32, y: u32| img[(4 * (y * w + x)) as usize..][..3].to_vec();

    // Translucent planes: the same picture whichever is drawn first.
    r.render(&cam, &[draw(1, &a, [0.9, 0.1, 0.1], 0.5), draw(2, &b, [0.1, 0.1, 0.9], 0.5)], &settings);
    let (_, _, ab) = r.read_pixels().unwrap();
    r.render(&cam, &[draw(2, &b, [0.1, 0.1, 0.9], 0.5), draw(1, &a, [0.9, 0.1, 0.1], 0.5)], &settings);
    let (_, _, ba) = r.read_pixels().unwrap();
    assert_eq!(ab, ba, "order-independent transparency");
    let centre = px(&ab, w / 2, h / 2);
    assert!(centre[0] > 40 && centre[2] > 40, "both planes show through: {centre:?}");
    // Translucent in front of opaque: the opaque color shows through.
    r.render(&cam, &[draw(1, &a, [0.9, 0.1, 0.1], 1.0), draw(2, &b, [0.1, 0.1, 0.9], 0.5)], &settings);
    let (_, _, mixed) = r.read_pixels().unwrap();
    let c = px(&mixed, w / 2, h / 2);
    assert!(c[0] > 60 && c[2] > 60, "red behind translucent blue: {c:?}");
    if let Ok(d) = std::env::var("AURAFX_DUMP") {
        let _ = image::save_buffer(format!("{d}/mixed.png"), &mixed, w, h, image::ColorType::Rgba8);
        let _ = image::save_buffer(format!("{d}/ab.png"), &ab, w, h, image::ColorType::Rgba8);
    }
    // Between the blue plane's edge and the red plane's edge only red shows.
    assert!(
        (w / 2..w).any(|x| {
            let p = px(&mixed, x, h / 2);
            p[0] > 150 && p[0] as i32 > p[2] as i32 + 80
        }),
        "only red outside the blue plane"
    );

    // Opaque points write depth: a cloud behind an opaque plane is hidden by it.
    let pts: Vec<f64> = (0..400).flat_map(|i| [((i % 20) as f64 - 9.5) / 10.0, ((i / 20) as f64 - 9.5) / 10.0, -0.5]).collect();
    let cloud = DataSet::PolyData(aurafx_data::PolyData {
        points: aurafx_data::Points::new_f64(pts),
        verts: {
            let mut c = aurafx_data::Cells::new();
            for i in 0..400u32 {
                c.push(aurafx_data::CellType::Vertex, &[i]);
            }
            c
        },
        ..Default::default()
    });
    let mut pd = DisplayProps::default();
    pd.representation = aurafx_pipeline::Representation::Points;
    pd.solid_color = [0.1, 0.9, 0.1];
    pd.point_size = 6.0;
    let points = NodeDraw { id: 3, mesh: Arc::new(tessellate::tessellate(&cloud, &pd)), display: pd, overlay: false };
    let covered = plane(0.0, 0.3);
    r.render(&cam, &[points, draw(4, &covered, [0.9, 0.9, 0.9], 1.0)], &settings);
    let (_, _, img) = r.read_pixels().unwrap();
    let greenish = |p: Vec<u8>| p[1] as i32 > p[0] as i32 + 40;
    let inside_plane = (w / 2 - 10..w / 2 + 10).flat_map(|x| (h / 2 - 10..h / 2 + 10).map(move |y| (x, y))).filter(|&(x, y)| greenish(px(&img, x, y))).count();
    assert_eq!(inside_plane, 0, "points behind the plane are hidden");
    assert!((0..w).flat_map(|x| (0..h).map(move |y| (x, y))).any(|(x, y)| greenish(px(&img, x, y))), "points outside it show");

    // Eye-Dome Lighting outlines silhouettes and leaves flat interiors nearly unchanged.
    let sphere = DataSet::PolyData(aurafx_filters::sources::sphere([0.0, 0.0, 0.0], 0.8, 64, 32));
    let mut cam2 = Camera::default();
    cam2.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &sphere.bounds());
    cam2.update_clip(&sphere.bounds());
    r.render(&cam2, &[draw(5, &sphere, [0.8, 0.8, 0.8], 1.0)], &settings);
    let (_, _, plain) = r.read_pixels().unwrap();
    let depth = r.read_depth().unwrap();
    r.render(&cam2, &[draw(5, &sphere, [0.8, 0.8, 0.8], 1.0)], &RenderSettings { edl: true, ..settings.clone() });
    let (_, _, edl) = r.read_pixels().unwrap();
    let lum = |img: &[u8], i: usize| img[4 * i] as f64 + img[4 * i + 1] as f64 + img[4 * i + 2] as f64;
    let n = (w * h) as usize;
    let rim: Vec<usize> =
        (0..n).filter(|&i| depth[i] < 1.0 && [i + 1, i - 1, i + w as usize, i - w as usize].iter().any(|&j| j < n && depth[j] >= 1.0)).collect();
    let inner: Vec<usize> = (0..n).filter(|&i| depth[i] < 1.0 && !rim.contains(&i)).collect();
    let darker = |set: &[usize]| set.iter().map(|&i| lum(&plain, i) - lum(&edl, i)).sum::<f64>() / set.len().max(1) as f64;
    eprintln!("EDL darkens the rim by {:.1}, the inside by {:.1}", darker(&rim), darker(&inner));
    assert!(darker(&rim) > 30.0 && darker(&inner) < darker(&rim) / 3.0);

    // The light kit lights the side away from the key light (fill and head lights).
    let side = |kit: bool, r: &mut Renderer| {
        r.render(&cam2, &[draw(5, &sphere, [0.8, 0.8, 0.8], 1.0)], &RenderSettings { light_kit: kit, ..settings.clone() });
        let (_, _, img) = r.read_pixels().unwrap();
        // Lower left of the sphere faces away from the key light (upper right).
        lum(&img, ((h / 2 + 25) * w + w / 2 - 25) as usize)
    };
    let (one, kit) = (side(false, &mut r), side(true, &mut r));
    eprintln!("shadow side: one light {one}, light kit {kit}");
    assert!(kit > one, "fill light brightens the shadow side");
}
