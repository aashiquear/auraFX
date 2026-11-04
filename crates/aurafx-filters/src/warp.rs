//! Warp by vector / scalar, and point-wise transforms.

use anyhow::{anyhow, Result};
use aurafx_data::*;

fn with_points(ds: &DataSet, f: impl Fn(&Points, &Attributes) -> Result<Points>) -> Result<DataSet> {
    let ds = crate::simplex::as_explicit(ds).into_owned();
    let (points, pd) = match &ds {
        DataSet::UnstructuredGrid(u) => (&u.points, &u.point_data),
        DataSet::PolyData(p) => (&p.points, &p.point_data),
        _ => return Err(anyhow!("warp: unsupported dataset")),
    };
    let new_points = f(points, pd)?;
    let mut out = ds.clone();
    match &mut out {
        DataSet::UnstructuredGrid(u) => u.points = new_points,
        DataSet::PolyData(p) => p.points = new_points,
        _ => unreachable!(),
    }
    Ok(out)
}

pub fn warp_by_vector(ds: &DataSet, array: &str, scale: f64) -> Result<DataSet> {
    with_points(ds, |points, pd| {
        let arr = pd.get(array).ok_or_else(|| anyhow!("warp: no point array {array:?}"))?;
        if arr.ncomp < 3 {
            return Err(anyhow!("warp: {array:?} is not a 3-vector"));
        }
        let n = points.len();
        let mut v = Vec::with_capacity(3 * n);
        for i in 0..n {
            let p = points.get(i);
            v.extend_from_slice(&[p[0] + scale * arr.get(i, 0), p[1] + scale * arr.get(i, 1), p[2] + scale * arr.get(i, 2)]);
        }
        Ok(Points::new_f64(v))
    })
}

pub fn warp_by_scalar(ds: &DataSet, array: &str, scale: f64, normal: [f64; 3], use_normals: bool) -> Result<DataSet> {
    with_points(ds, |points, pd| {
        let arr = pd.get(array).ok_or_else(|| anyhow!("warp: no point array {array:?}"))?;
        let normals = if use_normals { pd.active(AttributeKind::Normals) } else { None };
        let n = points.len();
        let mut v = Vec::with_capacity(3 * n);
        let dn = crate::slice::normalize(normal);
        for i in 0..n {
            let p = points.get(i);
            let s = scale * arr.magnitude(i);
            let nn = match normals {
                Some(na) => [na.get(i, 0), na.get(i, 1), na.get(i, 2)],
                None => dn,
            };
            v.extend_from_slice(&[p[0] + s * nn[0], p[1] + s * nn[1], p[2] + s * nn[2]]);
        }
        Ok(Points::new_f64(v))
    })
}
