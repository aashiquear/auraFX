//! Clip by implicit function or scalar: keeps the part where f >= 0 (or the opposite with `inside_out`).

use crate::attributes::cell_to_point;
use crate::interp::{PointBuilder, ScalarFn};
use crate::params::Association;
use crate::simplex::{decompose, parts, Simplex};
use anyhow::{anyhow, Result};
use aurafx_data::*;

pub fn clip_with(ds: &DataSet, f: &ScalarFn, inside_out: bool) -> Result<UnstructuredGrid> {
    let ds = crate::higher_order::as_linear(ds);
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow!("clip: unsupported dataset"))?;
    let n = points.len();
    let sign = if inside_out { -1.0 } else { 1.0 };
    let vals: Vec<f64> = {
        use rayon::prelude::*;
        (0..n).into_par_iter().with_min_len(8192).map(|i| sign * f.eval(i)).collect()
    };
    let edge_pt = |pb: &mut PointBuilder, a: u32, b: u32| -> u32 {
        let (va, vb) = (vals[a as usize], vals[b as usize]);
        pb.on_edge(a, b, (va / (va - vb)).clamp(0.0, 1.0))
    };
    // Prism pieces are split like cells (quad faces along the diagonal through their first
    // point by position) so pieces of neighbouring cells meet on shared faces.
    let push_wedge = |pb: &PointBuilder, cells: &mut Cells, parents: &mut Vec<usize>, w: [u32; 6], cid: usize| {
        for wt in crate::simplex::split_by(CellType::Wedge, |k| (crate::simplex::PointRank(pb.coords(w[k]), 0), pb.rank(w[k]))) {
            let mut t = [w[wt[0]], w[wt[1]], w[wt[2]], w[wt[3]]];
            let p = t.map(|i| pb.coords(i));
            let e = |k: usize| [p[k][0] - p[0][0], p[k][1] - p[0][1], p[k][2] - p[0][2]];
            let (a, b, c) = (e(1), e(2), e(3));
            if a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]) < 0.0 {
                t.swap(1, 2);
            }
            cells.push(CellType::Tetra, &t);
            parents.push(cid);
        }
    };
    let make = || (PointBuilder::new(points, pd), Cells::new(), Vec::<usize>::new());

    let chunks = crate::interp::par_cell_chunks(&ds, make, |st, cid, ty, pts, faces| {
        let (pb, cells, parents) = st;
        // Fast path: whole cell inside/outside.
        let all_in = pts.iter().all(|&p| vals[p as usize] >= 0.0);
        let all_out = pts.iter().all(|&p| vals[p as usize] < 0.0);
        if all_out {
            return;
        }
        if all_in && ty.is_linear() && ty != CellType::Polyhedron {
            let mapped: Vec<u32> = pts.iter().map(|&p| pb.keep(p)).collect();
            cells.push(ty, &mapped);
            parents.push(cid);
            return;
        }
        decompose(points, ty, pts, faces, |s| match s {
            Simplex::Tet(t) => {
                let inside: Vec<bool> = t.iter().map(|&p| vals[p as usize] >= 0.0).collect();
                let ins: Vec<u32> = (0..4).filter(|&k| inside[k]).map(|k| t[k]).collect();
                let outs: Vec<u32> = (0..4).filter(|&k| !inside[k]).map(|k| t[k]).collect();
                match ins.len() {
                    0 => {}
                    4 => {
                        let m: Vec<u32> = t.iter().map(|&p| pb.keep(p)).collect();
                        cells.push(CellType::Tetra, &m);
                        parents.push(cid);
                    }
                    1 => {
                        let a = ins[0];
                        let m = [pb.keep(a), edge_pt(pb, a, outs[0]), edge_pt(pb, a, outs[1]), edge_pt(pb, a, outs[2])];
                        cells.push(CellType::Tetra, &m);
                        parents.push(cid);
                    }
                    3 => {
                        let d = outs[0];
                        let w = [pb.keep(ins[0]), pb.keep(ins[1]), pb.keep(ins[2]), edge_pt(pb, ins[0], d), edge_pt(pb, ins[1], d), edge_pt(pb, ins[2], d)];
                        push_wedge(pb, cells, parents, w, cid);
                    }
                    _ => {
                        let (a, b, c, d) = (ins[0], ins[1], outs[0], outs[1]);
                        let w = [pb.keep(a), edge_pt(pb, a, c), edge_pt(pb, a, d), pb.keep(b), edge_pt(pb, b, c), edge_pt(pb, b, d)];
                        push_wedge(pb, cells, parents, w, cid);
                    }
                }
            }
            Simplex::Tri(t) => {
                let inside: Vec<bool> = t.iter().map(|&p| vals[p as usize] >= 0.0).collect();
                let ins: Vec<u32> = (0..3).filter(|&k| inside[k]).map(|k| t[k]).collect();
                let outs: Vec<u32> = (0..3).filter(|&k| !inside[k]).map(|k| t[k]).collect();
                match ins.len() {
                    0 => {}
                    3 => {
                        let m: Vec<u32> = t.iter().map(|&p| pb.keep(p)).collect();
                        cells.push(CellType::Triangle, &m);
                        parents.push(cid);
                    }
                    1 => {
                        let a = ins[0];
                        let m = [pb.keep(a), edge_pt(pb, a, outs[0]), edge_pt(pb, a, outs[1])];
                        cells.push(CellType::Triangle, &m);
                        parents.push(cid);
                    }
                    _ => {
                        let (a, b, c) = (ins[0], ins[1], outs[0]);
                        let q = [pb.keep(a), pb.keep(b), edge_pt(pb, b, c), edge_pt(pb, a, c)];
                        cells.push(CellType::Quad, &q);
                        parents.push(cid);
                    }
                }
            }
            Simplex::Line([a, b]) => {
                let (ia, ib) = (vals[a as usize] >= 0.0, vals[b as usize] >= 0.0);
                match (ia, ib) {
                    (true, true) => {
                        cells.push(CellType::Line, &[pb.keep(a), pb.keep(b)]);
                        parents.push(cid);
                    }
                    (true, false) => {
                        cells.push(CellType::Line, &[pb.keep(a), edge_pt(pb, a, b)]);
                        parents.push(cid);
                    }
                    (false, true) => {
                        cells.push(CellType::Line, &[edge_pt(pb, a, b), pb.keep(b)]);
                        parents.push(cid);
                    }
                    _ => {}
                }
            }
            Simplex::Vert(p) => {
                if vals[p as usize] >= 0.0 {
                    cells.push(CellType::Vertex, &[pb.keep(p)]);
                    parents.push(cid);
                }
            }
        });
    });
    let mut builders = Vec::with_capacity(chunks.len());
    let mut rest = Vec::with_capacity(chunks.len());
    for (pb, c, p) in chunks {
        builders.push(pb);
        rest.push((c, p));
    }
    let (pb, remaps) = PointBuilder::merge(points, pd, builders);
    let cells = crate::interp::concat_remapped(&rest.iter().zip(&remaps).map(|((c, _), r)| (c, r.as_slice())).collect::<Vec<_>>());
    let parents: Vec<usize> = rest.into_iter().flat_map(|(_, p)| p).collect();
    let (points, point_data) = pb.finish();
    Ok(UnstructuredGrid { points, cells, point_data, cell_data: cd.gather(&parents), field_data: fd.clone() })
}

pub fn clip_plane(ds: &DataSet, origin: [f64; 3], normal: [f64; 3], inside_out: bool) -> Result<UnstructuredGrid> {
    let ds = crate::higher_order::as_linear(ds);
    let (points, ..) = parts(&ds).ok_or_else(|| anyhow!("clip: unsupported dataset"))?;
    clip_with(&ds, &ScalarFn::Plane { origin, normal: crate::slice::normalize(normal), points }, inside_out)
}

pub fn clip_sphere(ds: &DataSet, center: [f64; 3], radius: f64, inside_out: bool) -> Result<UnstructuredGrid> {
    let ds = crate::higher_order::as_linear(ds);
    let (points, ..) = parts(&ds).ok_or_else(|| anyhow!("clip: unsupported dataset"))?;
    // Sphere function is negative inside; "keep inside" is the natural default, so flip.
    clip_with(&ds, &ScalarFn::Sphere { center, radius, points }, !inside_out)
}

pub fn clip_box(ds: &DataSet, min: [f64; 3], max: [f64; 3], inside_out: bool) -> Result<UnstructuredGrid> {
    let ds = crate::higher_order::as_linear(ds);
    let (points, ..) = parts(&ds).ok_or_else(|| anyhow!("clip: unsupported dataset"))?;
    clip_with(&ds, &ScalarFn::Box { min, max, points }, !inside_out)
}

pub fn clip_scalar(ds: &DataSet, assoc: Association, array: &str, comp: Option<usize>, value: f64, inside_out: bool) -> Result<UnstructuredGrid> {
    let ds = crate::higher_order::as_linear(ds);
    let ds: std::borrow::Cow<DataSet> = if assoc == Association::Cells { std::borrow::Cow::Owned(cell_to_point(&ds, &[array.to_string()])?) } else { ds };
    let (_, pd, ..) = parts(&ds).ok_or_else(|| anyhow!("clip: unsupported dataset"))?;
    let arr = pd.get(array).ok_or_else(|| anyhow!("clip: no point array {array:?}"))?;
    clip_with(&ds, &ScalarFn::Array { arr, comp, offset: value }, inside_out)
}
