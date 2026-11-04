//! Bounding-box outline.

use aurafx_data::*;

pub fn outline(ds: &DataSet) -> PolyData {
    let b = ds.bounds();
    if b.is_empty() {
        return PolyData::default();
    }
    let (lo, hi) = (b.min, b.max);
    let corners = [
        [lo[0], lo[1], lo[2]],
        [hi[0], lo[1], lo[2]],
        [hi[0], hi[1], lo[2]],
        [lo[0], hi[1], lo[2]],
        [lo[0], lo[1], hi[2]],
        [hi[0], lo[1], hi[2]],
        [hi[0], hi[1], hi[2]],
        [lo[0], hi[1], hi[2]],
    ];
    let mut pts = Vec::with_capacity(24);
    for c in corners {
        pts.extend_from_slice(&c);
    }
    let mut lines = Cells::new();
    for e in [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]] {
        lines.push(CellType::Line, &[e[0], e[1]]);
    }
    PolyData { points: Points::new_f64(pts), lines, ..Default::default() }
}
