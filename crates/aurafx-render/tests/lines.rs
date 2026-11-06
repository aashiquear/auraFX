//! Wide lines: a line drawn 1, 3 and 7 pixels wide covers about that many pixels across.
use aurafx_data::*;
use aurafx_pipeline::{DisplayProps, Representation};
use aurafx_render::*;
use std::sync::Arc;

#[test]
fn line_width_sets_pixels_across() {
    let mut r = match headless_with_samples(200, 200, 1) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GPU adapter: {e}");
            return;
        }
    };
    // A horizontal line, and a bent polyline whose joint must not leave a gap.
    let mut cells = Cells::new();
    cells.push(CellType::Line, &[0, 1]);
    cells.push(CellType::PolyLine, &[2, 3, 4]);
    let pts = vec![-1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, -0.6, 0.0, 0.0, -0.9, 0.0, 1.0, -0.6, 0.0];
    let ds = DataSet::PolyData(PolyData { points: Points::new_f64(pts), lines: cells, ..Default::default() });
    let mut cam = Camera::default();
    cam.set_view([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &ds.bounds());
    let mut across = Vec::new();
    for width in [1.0f32, 3.0, 7.0] {
        let d = DisplayProps { representation: Representation::Surface, color_by: None, solid_color: [1.0, 0.0, 0.0], lighting: false, line_width: width, ..Default::default() };
        let mesh = Arc::new(tessellate::tessellate(&ds, &d));
        r.render(&cam, &[NodeDraw { id: 1, mesh, display: d, overlay: false }], &RenderSettings::default());
        let (w, h, px) = r.read_pixels().unwrap();
        let red = |x: u32, y: u32| {
            let p = &px[((y * w + x) * 4) as usize..][..4];
            p[0] > 150 && p[1] < 100 && p[2] < 100
        };
        // Pixels across the straight line in the middle column.
        let n = (0..h).filter(|&y| red(w / 2, y) && y < h / 2 + 10).count();
        // The polyline is unbroken: every column between its ends has red below the line.
        let gaps = (w / 4..3 * w / 4).filter(|&x| !(h / 2 + 10..h).any(|y| red(x, y))).count();
        eprintln!("width {width}: {n} pixels across, {gaps} gap columns");
        assert_eq!(gaps, 0, "width {width}");
        across.push(n);
    }
    assert!(across[0] <= 2, "{across:?}");
    assert!((2..=4).contains(&across[1]), "{across:?}");
    assert!((6..=8).contains(&across[2]), "{across:?}");
}
