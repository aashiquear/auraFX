//! Slice (cut) by an implicit function, and the shared simplex-cutting core used by Contour.

use crate::interp::{PointBuilder, ScalarFn};
use crate::simplex::{decompose, parts, Simplex};
use anyhow::Result;
use aurafx_data::*;

/// Cut every simplex at the zero level of `f`. Output: triangles for tets, lines for
/// triangles, vertices for lines. Point data is interpolated; cell data copied from parents.
pub fn cut_at_zero(ds: &DataSet, f: &ScalarFn, merge_points: bool) -> Result<PolyData> {
    use rayon::prelude::*;
    let ds = crate::higher_order::as_linear(ds);
    let (points, pd, cd, fd) = parts(&ds).ok_or_else(|| anyhow::anyhow!("slice: unsupported dataset"))?;
    let n = points.len();
    let vals: Vec<f64> = (0..n).into_par_iter().with_min_len(8192).map(|i| f.eval(i)).collect();
    let _ = merge_points;

    struct Part<'a> {
        pb: PointBuilder<'a>,
        polys: Cells,
        lines: Cells,
        verts: Cells,
        pp: Vec<usize>,
        pl: Vec<usize>,
        pv: Vec<usize>,
    }
    let edge_pt = |pb: &mut PointBuilder, a: u32, b: u32| -> u32 {
        let (va, vb) = (vals[a as usize], vals[b as usize]);
        let t = va / (va - vb);
        pb.on_edge(a, b, t.clamp(0.0, 1.0))
    };
    let make = || Part { pb: PointBuilder::new(points, pd), polys: Cells::new(), lines: Cells::new(), verts: Cells::new(), pp: vec![], pl: vec![], pv: vec![] };
    let chunks = crate::interp::par_cell_chunks(&ds, make, |st, cid, ty, pts, faces| {
        decompose(points, ty, pts, faces, |s| match s {
            Simplex::Tet(t) => {
                let inside = t.map(|p| vals[p as usize] >= 0.0);
                let n_in = inside.iter().filter(|&&b| b).count();
                if n_in == 0 || n_in == 4 {
                    return;
                }
                let mut ins = [0u32; 4];
                let mut outs = [0u32; 4];
                let (mut ni, mut no) = (0, 0);
                for k in 0..4 {
                    if inside[k] {
                        ins[ni] = t[k];
                        ni += 1;
                    } else {
                        outs[no] = t[k];
                        no += 1;
                    }
                }
                if n_in == 1 || n_in == 3 {
                    let (apex, base) = if n_in == 1 { (ins[0], &outs) } else { (outs[0], &ins) };
                    let tri = [edge_pt(&mut st.pb, apex, base[0]), edge_pt(&mut st.pb, apex, base[1]), edge_pt(&mut st.pb, apex, base[2])];
                    st.polys.push(CellType::Triangle, &tri);
                    st.pp.push(cid);
                } else {
                    let (a, b, c, d) = (ins[0], ins[1], outs[0], outs[1]);
                    let q = [edge_pt(&mut st.pb, a, c), edge_pt(&mut st.pb, a, d), edge_pt(&mut st.pb, b, d), edge_pt(&mut st.pb, b, c)];
                    st.polys.push(CellType::Quad, &q);
                    st.pp.push(cid);
                }
            }
            Simplex::Tri(t) => {
                let inside = t.map(|p| vals[p as usize] >= 0.0);
                let n_in = inside.iter().filter(|&&b| b).count();
                if n_in == 0 || n_in == 3 {
                    return;
                }
                let ins: Vec<u32> = (0..3).filter(|&k| inside[k]).map(|k| t[k]).collect();
                let outs: Vec<u32> = (0..3).filter(|&k| !inside[k]).map(|k| t[k]).collect();
                let (apex, base) = if n_in == 1 { (ins[0], &outs) } else { (outs[0], &ins) };
                let l = [edge_pt(&mut st.pb, apex, base[0]), edge_pt(&mut st.pb, apex, base[1])];
                st.lines.push(CellType::Line, &l);
                st.pl.push(cid);
            }
            Simplex::Line([a, b]) => {
                let (va, vb) = (vals[a as usize], vals[b as usize]);
                if (va >= 0.0) != (vb >= 0.0) {
                    let p = edge_pt(&mut st.pb, a, b);
                    st.verts.push(CellType::Vertex, &[p]);
                    st.pv.push(cid);
                }
            }
            Simplex::Vert(_) => {}
        });
    });
    let mut builders = Vec::with_capacity(chunks.len());
    let mut rest = Vec::with_capacity(chunks.len());
    for c in chunks {
        builders.push(c.pb);
        rest.push((c.polys, c.lines, c.verts, c.pp, c.pl, c.pv));
    }
    let (pb, remaps) = PointBuilder::merge(points, pd, builders);
    let cat = |f: fn(&(Cells, Cells, Cells, Vec<usize>, Vec<usize>, Vec<usize>)) -> &Cells| {
        crate::interp::concat_remapped(&rest.iter().zip(&remaps).map(|(r, m)| (f(r), m.as_slice())).collect::<Vec<_>>())
    };
    let (polys, lines, verts) = (cat(|r| &r.0), cat(|r| &r.1), cat(|r| &r.2));
    let (mut pp, mut pl, mut pv) = (Vec::new(), Vec::new(), Vec::new());
    for (_, _, _, a, b, c) in rest {
        pp.extend(a);
        pl.extend(b);
        pv.extend(c);
    }
    let (points, point_data) = pb.finish();
    let mut parents = pv;
    parents.extend(pl);
    parents.extend(pp);
    Ok(PolyData { points, verts, lines, polys, strips: Cells::new(), point_data, cell_data: cd.gather(&parents), field_data: fd.clone() })
}

pub fn slice_plane(ds: &DataSet, origin: [f64; 3], normal: [f64; 3]) -> Result<PolyData> {
    let ds = crate::higher_order::as_linear(ds);
    let (points, ..) = parts(&ds).ok_or_else(|| anyhow::anyhow!("slice: unsupported dataset"))?;
    let n = normalize(normal);
    cut_at_zero(&ds, &ScalarFn::Plane { origin, normal: n, points }, true)
}

pub fn slice_sphere(ds: &DataSet, center: [f64; 3], radius: f64) -> Result<PolyData> {
    let ds = crate::higher_order::as_linear(ds);
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
