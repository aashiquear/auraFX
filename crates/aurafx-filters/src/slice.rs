//! Slice (cut) by an implicit function, and the shared simplex-cutting core used by Contour.

use crate::interp::{PointBuilder, ScalarFn};
use crate::simplex::{as_explicit, decompose, for_each_cell, parts, Simplex};
use anyhow::Result;
use aurafx_data::*;

/// Cut every simplex at the zero level of `f`. Output: triangles for tets, lines for
/// triangles, vertices for lines. Point data is interpolated; cell data copied from parents.
pub fn cut_at_zero(ds: &DataSet, f: &ScalarFn, merge_points: bool) -> Result<PolyData> {
    let ds = as_explicit(ds);
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow::anyhow!("slice: unsupported dataset"))?;
    let n = points.len();
    let vals: Vec<f64> = (0..n).map(|i| f.eval(i)).collect();
    let mut pb = PointBuilder::new(points, pd);
    let mut polys = Cells::new();
    let mut lines = Cells::new();
    let mut verts = Cells::new();
    let mut parents_p = Vec::new();
    let mut parents_l = Vec::new();
    let mut parents_v = Vec::new();
    let _ = merge_points;

    let edge_pt = |pb: &mut PointBuilder, a: u32, b: u32| -> u32 {
        let (va, vb) = (vals[a as usize], vals[b as usize]);
        let t = va / (va - vb);
        pb.on_edge(a, b, t.clamp(0.0, 1.0))
    };

    for_each_cell(&ds, |cid, ty, pts, faces| {
        decompose(ty, pts, faces, |s| match s {
            Simplex::Tet(t) => {
                let inside: Vec<bool> = t.iter().map(|&p| vals[p as usize] >= 0.0).collect();
                let n_in = inside.iter().filter(|&&b| b).count();
                if n_in == 0 || n_in == 4 {
                    return;
                }
                let ins: Vec<u32> = (0..4).filter(|&k| inside[k]).map(|k| t[k]).collect();
                let outs: Vec<u32> = (0..4).filter(|&k| !inside[k]).map(|k| t[k]).collect();
                if n_in == 1 || n_in == 3 {
                    let (apex, base) = if n_in == 1 { (ins[0], &outs) } else { (outs[0], &ins) };
                    let tri = [edge_pt(&mut pb, apex, base[0]), edge_pt(&mut pb, apex, base[1]), edge_pt(&mut pb, apex, base[2])];
                    polys.push(CellType::Triangle, &tri);
                    parents_p.push(cid);
                } else {
                    let (a, b, c, d) = (ins[0], ins[1], outs[0], outs[1]);
                    let q = [edge_pt(&mut pb, a, c), edge_pt(&mut pb, a, d), edge_pt(&mut pb, b, d), edge_pt(&mut pb, b, c)];
                    polys.push(CellType::Quad, &q);
                    parents_p.push(cid);
                }
            }
            Simplex::Tri(t) => {
                let inside: Vec<bool> = t.iter().map(|&p| vals[p as usize] >= 0.0).collect();
                let n_in = inside.iter().filter(|&&b| b).count();
                if n_in == 0 || n_in == 3 {
                    return;
                }
                let ins: Vec<u32> = (0..3).filter(|&k| inside[k]).map(|k| t[k]).collect();
                let outs: Vec<u32> = (0..3).filter(|&k| !inside[k]).map(|k| t[k]).collect();
                let (apex, base) = if n_in == 1 { (ins[0], &outs) } else { (outs[0], &ins) };
                let l = [edge_pt(&mut pb, apex, base[0]), edge_pt(&mut pb, apex, base[1])];
                lines.push(CellType::Line, &l);
                parents_l.push(cid);
            }
            Simplex::Line([a, b]) => {
                let (va, vb) = (vals[a as usize], vals[b as usize]);
                if (va >= 0.0) != (vb >= 0.0) {
                    let p = edge_pt(&mut pb, a, b);
                    verts.push(CellType::Vertex, &[p]);
                    parents_v.push(cid);
                }
            }
            Simplex::Vert(_) => {}
        });
    });
    let (points, point_data) = pb.finish();
    let mut parents = parents_v;
    parents.extend(parents_l);
    parents.extend(parents_p);
    Ok(PolyData { points, verts, lines, polys, strips: Cells::new(), point_data, cell_data: cd.gather(&parents), field_data: fd.clone() })
}

pub fn slice_plane(ds: &DataSet, origin: [f64; 3], normal: [f64; 3]) -> Result<PolyData> {
    let ds = as_explicit(ds);
    let (points, ..) = parts(&ds).ok_or_else(|| anyhow::anyhow!("slice: unsupported dataset"))?;
    let n = normalize(normal);
    cut_at_zero(&ds, &ScalarFn::Plane { origin, normal: n, points }, true)
}

pub fn slice_sphere(ds: &DataSet, center: [f64; 3], radius: f64) -> Result<PolyData> {
    let ds = as_explicit(ds);
    let (points, ..) = parts(&ds).ok_or_else(|| anyhow::anyhow!("slice: unsupported dataset"))?;
    cut_at_zero(&ds, &ScalarFn::Sphere { center, radius, points }, true)
}

pub fn normalize(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l == 0.0 {
        [0.0, 0.0, 1.0]
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}
