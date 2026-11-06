//! Affine transform as ParaView's Transform: scale, rotate (Y, X, Z in degrees), translate.

use anyhow::{anyhow, Result};
use aurafx_data::*;

/// ParaView's Transform (vtkPVTransform): scale, then rotate about Y, X and Z (degrees, in that
/// order: R = Rz Rx Ry), then translate.
pub fn affine(translate: [f64; 3], rotate_deg: [f64; 3], scale: [f64; 3]) -> impl Fn([f64; 3]) -> [f64; 3] {
    let (rx, ry, rz) = (rotate_deg[0].to_radians(), rotate_deg[1].to_radians(), rotate_deg[2].to_radians());
    let (cx, sx, cy, sy, cz, sz) = (rx.cos(), rx.sin(), ry.cos(), ry.sin(), rz.cos(), rz.sin());
    let rxm = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let rym = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rzm = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let mul = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| -> [[f64; 3]; 3] { std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum())) };
    let m = mul(mul(rzm, rxm), rym);
    move |p: [f64; 3]| -> [f64; 3] {
        let s = [p[0] * scale[0], p[1] * scale[1], p[2] * scale[2]];
        std::array::from_fn(|i| m[i][0] * s[0] + m[i][1] * s[1] + m[i][2] * s[2] + translate[i])
    }
}

pub fn transform(ds: &DataSet, translate: [f64; 3], rotate_deg: [f64; 3], scale: [f64; 3]) -> Result<DataSet> {
    let apply = affine(translate, rotate_deg, scale);
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
