//! Affine transform: scale, rotate (XYZ Euler, degrees), translate.

use anyhow::{anyhow, Result};
use aurafx_data::*;

pub fn transform(ds: &DataSet, translate: [f64; 3], rotate_deg: [f64; 3], scale: [f64; 3]) -> Result<DataSet> {
    let (rx, ry, rz) = (rotate_deg[0].to_radians(), rotate_deg[1].to_radians(), rotate_deg[2].to_radians());
    let (cx, sx, cy, sy, cz, sz) = (rx.cos(), rx.sin(), ry.cos(), ry.sin(), rz.cos(), rz.sin());
    // R = Rz * Ry * Rx
    let m = [[cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx], [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx], [-sy, cy * sx, cy * cx]];
    let apply = |p: [f64; 3]| -> [f64; 3] {
        let s = [p[0] * scale[0], p[1] * scale[1], p[2] * scale[2]];
        [
            m[0][0] * s[0] + m[0][1] * s[1] + m[0][2] * s[2] + translate[0],
            m[1][0] * s[0] + m[1][1] * s[1] + m[1][2] * s[2] + translate[1],
            m[2][0] * s[0] + m[2][1] * s[1] + m[2][2] * s[2] + translate[2],
        ]
    };
    let ds = crate::simplex::as_explicit(ds).into_owned();
    let mut out = ds;
    let pts = match &mut out {
        DataSet::UnstructuredGrid(u) => &mut u.points,
        DataSet::PolyData(p) => &mut p.points,
        _ => return Err(anyhow!("transform: unsupported dataset")),
    };
    let n = pts.len();
    let mut v = Vec::with_capacity(3 * n);
    for i in 0..n {
        v.extend_from_slice(&apply(pts.get(i)));
    }
    *pts = Points::new_f64(v);
    Ok(out)
}
