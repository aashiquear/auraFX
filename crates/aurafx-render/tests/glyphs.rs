//! 3D Glyphs: one shape drawn per point by the GPU, turned along a vector array and sized by an
//! array; it draws where the points are and needs far less memory than the same arrows as
//! geometry.
use aurafx_data::*;
use aurafx_pipeline::{DisplayProps, GlyphShape, Representation};
use aurafx_render::*;
use std::sync::Arc;

#[test]
fn glyphs_are_drawn_per_point() {
    // Five points in a row along x, vectors pointing up (+y), sizes 1..5: cones growing to the right.
    let n = 5;
    let pts: Vec<f64> = (0..n).flat_map(|i| [i as f64 * 2.0, 0.0, 0.0]).collect();
    let mut pd = PolyData { points: Points::new_f64(pts), ..Default::default() };
    pd.point_data.insert(Array::new("V", 3, ArrayData::F64((0..n).flat_map(|_| [0.0, 1.0, 0.0]).collect())));
    pd.point_data.insert(Array::scalars_f64("s", (1..=n).map(|i| i as f64).collect()));
    let ds = DataSet::PolyData(pd);
    let d = DisplayProps {
        representation: Representation::Glyphs,
        // Cones: solid, so even the smallest is several pixels wide.
        glyph_shape: GlyphShape::Cone,
        glyph_orient: "V".into(),
        glyph_scale_by: "s".into(),
        glyph_scale: 1.5,
        color_by: None,
        solid_color: [1.0, 0.0, 0.0],
        lighting: false,
        ..Default::default()
    };
    let mesh = tessellate::tessellate(&ds, &d);
    let g = mesh.glyphs.as_ref().expect("glyphs");
    assert_eq!(g.instances.len(), n);
    assert!(mesh.tris.is_empty(), "no copies of the shape");
    // The largest is 1.5 long, the smallest a fifth of that; all point up.
    assert!((g.instances[4][7] - 1.5).abs() < 1e-6 && (g.instances[0][7] - 0.3).abs() < 1e-6);
    assert!(g.instances.iter().all(|i| (i[5] - 1.0).abs() < 1e-6));

    let Ok(mut r) = headless_with_samples(240, 160, 1) else {
        eprintln!("no GPU adapter");
        return;
    };
    let mut cam = Camera::default();
    let mut b = ds.bounds();
    b.max[1] = 1.5;
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &b);
    r.render(&cam, &[NodeDraw { id: 1, mesh: Arc::new(mesh), display: d, overlay: false }], &RenderSettings::default());
    let (w, h, px) = r.read_pixels().unwrap();
    let red = |x: u32, y: u32| {
        let p = &px[((y * w + x) * 4) as usize..][..4];
        p[0] > 150 && p[1] < 100 && p[2] < 100
    };
    // Arrows: runs of columns with red, left to right; each one's height in rows.
    let mut heights = Vec::new();
    let mut run: Option<(u32, u32)> = None;
    for x in 0..=w {
        let any = x < w && (0..h).any(|y| red(x, y));
        match (any, run) {
            (true, None) => run = Some((x, x)),
            (true, Some((a, _))) => run = Some((a, x)),
            (false, Some((a, b))) => {
                heights.push((0..h).filter(|&y| (a..=b).any(|x| red(x, y))).count());
                run = None;
            }
            _ => {}
        }
    }
    assert_eq!(heights.len(), n, "one arrow per point: {heights:?}");
    eprintln!("arrow heights in pixels: {heights:?}");
    assert!(heights.iter().all(|&v| v > 0), "{heights:?}");
    assert!(heights.windows(2).all(|p| p[1] >= p[0]), "{heights:?}");
    assert!(heights[4] > 3 * heights[0], "{heights:?}");
}

fn glyph_display() -> DisplayProps {
    DisplayProps {
        representation: Representation::Glyphs,
        glyph_orient: "V".into(),
        glyph_scale_by: "s".into(),
        glyph_scale: 2.0,
        color_by: None,
        ..Default::default()
    }
}

fn points_with(sizes: &[f64], vectors: &[[f64; 3]], x0: f64) -> DataSet {
    let n = sizes.len();
    let mut pd = PolyData { points: Points::new_f64((0..n).flat_map(|i| [x0 + i as f64, 0.0, 0.0]).collect()), ..Default::default() };
    pd.point_data.insert(Array::new("V", 3, ArrayData::F64(vectors.iter().flatten().copied().collect())));
    pd.point_data.insert(Array::scalars_f64("s", sizes.to_vec()));
    DataSet::PolyData(pd)
}

/// The blocks of an object share one scale: the same magnitude gives the same length in every
/// block. Points whose vector or size is not a number get no glyph.
#[test]
fn blocks_share_one_scale() {
    let up = [0.0, 1.0, 0.0];
    let mut mb = MultiBlock::default();
    mb.blocks.push(("small".into(), Some(points_with(&[1.0, 2.0], &[up, up], 0.0))));
    mb.blocks.push(("large".into(), Some(points_with(&[2.0, 4.0, f64::NAN, 3.0, 1.0], &[up, up, up, [f64::NAN, 0.0, 0.0], [f64::INFINITY, 0.0, 0.0]], 10.0))));
    let mesh = tessellate::tessellate(&DataSet::MultiBlock(mb), &glyph_display());
    let g = mesh.glyphs.as_ref().expect("glyphs");
    assert_eq!(g.stride, 1);
    let sizes: Vec<f32> = g.instances.iter().map(|i| i[7]).collect();
    // 4 is the largest (2.0 long); the 2 of either block is half of that.
    assert_eq!(sizes, vec![0.5, 1.0, 1.0, 2.0], "sizes over both blocks; the three that are not numbers are left out");
    assert!(g.instances.iter().all(|i| i.iter().all(|v| v.is_finite())));
}

/// More points than glyphs are drawn for: every n-th point gets one, evenly over the blocks.
#[test]
fn glyphs_are_capped() {
    let per_block = tessellate::MAX_GLYPHS * 5 / 8;
    let block = |x0: f64| {
        let pd = PolyData { points: Points::new_f32((0..per_block).flat_map(|i| [x0 as f32 + i as f32 * 1e-3, 0.0, 0.0]).collect()), ..Default::default() };
        DataSet::PolyData(pd)
    };
    let mut mb = MultiBlock::default();
    mb.blocks.push(("a".into(), Some(block(0.0))));
    mb.blocks.push(("b".into(), Some(block(5000.0))));
    let d = DisplayProps { representation: Representation::Glyphs, color_by: None, ..Default::default() };
    let mesh = tessellate::tessellate(&DataSet::MultiBlock(mb), &d);
    let g = mesh.glyphs.as_ref().expect("glyphs");
    assert_eq!(g.stride, 2, "1.25 times the most: every second point");
    assert_eq!(g.instances.len(), per_block, "half of both blocks' points");
    assert!(g.instances.len() <= tessellate::MAX_GLYPHS);
    // Both blocks are thinned alike (their points lie left and right of the middle).
    let left = g.instances.iter().filter(|i| i[0] < 0.0).count();
    assert!(left.abs_diff(g.instances.len() - left) <= 1, "{left} of {}", g.instances.len());
    // The automatic size follows the glyphs drawn, one size for all.
    assert!(g.instances.iter().all(|i| i[7] == g.instances[0][7] && i[7] > 0.0));
}

#[test]
fn glyph_memory_compared_with_geometry() {
    // A million arrows: instances are 32 bytes each, the shape is drawn from one copy.
    let shape = tessellate::glyph_shape(GlyphShape::Arrow);
    let per_copy = shape.len() * std::mem::size_of::<tessellate::Vertex>();
    let per_instance = 32;
    eprintln!("an arrow as geometry: {per_copy} bytes; as an instance: {per_instance} bytes");
    assert!(per_copy > 50 * per_instance);
}

/// Frame times on a big surface: whole view, zoomed in (chunks outside the view skipped), and
/// while moving (coarse copy). Run with --ignored in release.
#[test]
#[ignore]
fn big_surface_frame_times() {
    let src = aurafx_filters::find("wavelet").unwrap();
    let mut p = src.default_params();
    p.0.insert("half_size".into(), aurafx_filters::ParamValue::Int(220));
    let img = src.run(None, &p).unwrap();
    let c = aurafx_filters::contour::contour(&img, aurafx_filters::Association::Points, "RTData", None, &[110.0, 160.0]).unwrap();
    let ds = DataSet::PolyData(c);
    let d = DisplayProps { color_by: None, ..Default::default() };
    let mesh = Arc::new(tessellate::tessellate(&ds, &d));
    eprintln!("{} triangles", mesh.tris.len() / 3);
    let Ok(mut r) = headless_with_samples(1280, 800, 4) else { return };
    let draws = [NodeDraw { id: 1, mesh: mesh.clone(), display: d, overlay: false }];
    let mut cam = Camera::default();
    cam.set_view([1.0, 0.6, 0.4], [0.0, 0.0, 1.0], &ds.bounds());
    let time = |r: &mut Renderer, cam: &Camera, moving: bool| {
        let s = RenderSettings { interacting: moving, ..Default::default() };
        r.render(cam, &draws, &s);
        r.read_pixels().unwrap();
        let t = std::time::Instant::now();
        for _ in 0..10 {
            r.render(cam, &draws, &s);
            r.read_pixels().unwrap();
        }
        t.elapsed().as_secs_f64() * 100.0
    };
    let whole = time(&mut r, &cam, false);
    // Wait for the coarse copy.
    for _ in 0..200 {
        r.render(&cam, &draws, &RenderSettings { interacting: true, ..Default::default() });
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let moving = time(&mut r, &cam, true);
    let mut near = cam.clone();
    near.zoom(12.0);
    let zoomed = time(&mut r, &near, false);
    eprintln!("ms per frame: whole {whole:.1}, moving (coarse) {moving:.1}, zoomed in {zoomed:.1}");
}
