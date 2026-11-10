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

/// Point `i` of a grid (image, rectilinear or structured), as its conversion to a mesh gives it.
fn grid_point(ds: &DataSet, i: usize) -> [f64; 3] {
    match ds {
        DataSet::ImageData(d) => {
            let [nx, ny, _] = d.dims();
            d.point(i % nx, (i / nx) % ny, i / (nx * ny))
        }
        DataSet::RectilinearGrid(g) => {
            let [nx, ny, _] = g.dims();
            [g.x[i % nx], g.y[(i / nx) % ny], g.z[i / (nx * ny)]]
        }
        DataSet::StructuredGrid(g) => g.points.get(i),
        _ => [0.0; 3],
    }
}

/// The cells of a 3-D grid (image, rectilinear or structured) whose corners do not all have the
/// same `side` (a bit per cut: which side of each cut a point is on), as an unstructured grid
/// with the cells and points in the grid's order: what cutting the whole grid would touch, built
/// without the rest, so a slice or contour of a big grid needs memory for the cells it crosses
/// only. None for other data, or when most cells cross.
///
/// `candidates` (from the GPU): the only cells that can cross, in increasing order; the exact
/// test then runs on those alone.
pub(crate) fn crossing_cells(ds: &DataSet, side: &(dyn Fn(usize) -> u64 + Sync), candidates: Option<Vec<u32>>) -> Option<DataSet> {
    use rayon::prelude::*;
    let (dims, pd, cd, fd) = match ds {
        DataSet::ImageData(d) => (d.dims(), &d.point_data, &d.cell_data, &d.field_data),
        DataSet::RectilinearGrid(d) => (d.dims(), &d.point_data, &d.cell_data, &d.field_data),
        DataSet::StructuredGrid(d) => (d.dims(), &d.point_data, &d.cell_data, &d.field_data),
        _ => return None,
    };
    let [nx, ny, nz] = dims;
    if nx < 2 || ny < 2 || nz < 2 {
        return None;
    }
    let idx = |i: usize, j: usize, k: usize| i + nx * (j + ny * k);
    let corners = |c: usize| -> [usize; 8] {
        let (i, j, k) = (c % (nx - 1), (c / (nx - 1)) % (ny - 1), c / ((nx - 1) * (ny - 1)));
        [
            idx(i, j, k),
            idx(i + 1, j, k),
            idx(i + 1, j + 1, k),
            idx(i, j + 1, k),
            idx(i, j, k + 1),
            idx(i + 1, j, k + 1),
            idx(i + 1, j + 1, k + 1),
            idx(i, j + 1, k + 1),
        ]
    };
    let n_cells = (nx - 1) * (ny - 1) * (nz - 1);
    let cells: Vec<usize> = match candidates {
        // Only the cells the GPU found, tested exactly.
        Some(c) => c
            .into_par_iter()
            .with_min_len(1024)
            .map(|c| c as usize)
            .filter(|&c| {
                let p = corners(c);
                let first = side(p[0]);
                p[1..].iter().any(|&q| side(q) != first)
            })
            .collect(),
        None => {
            let sides: Vec<u64> = (0..nx * ny * nz).into_par_iter().with_min_len(8192).map(side).collect();
            (0..n_cells)
                .into_par_iter()
                .with_min_len(4096)
                .filter(|&c| {
                    let p = corners(c);
                    let first = sides[p[0]];
                    p.iter().any(|&q| sides[q] != first)
                })
                .collect()
        }
    };
    if cells.len() * 2 > n_cells {
        return None;
    }
    let mut used: Vec<usize> = cells.iter().flat_map(|&c| corners(c)).collect();
    used.par_sort_unstable();
    used.dedup();
    let mut out = Cells::with_capacity(cells.len(), cells.len() * 8);
    for &c in &cells {
        let ids = corners(c).map(|q| used.binary_search(&q).unwrap() as u32);
        out.push(CellType::Hexahedron, &ids);
    }
    // Coordinates as the grid's conversion makes them (64-bit; structured grids keep theirs).
    let points = match ds {
        DataSet::StructuredGrid(g) => g.points.gather(&used),
        _ => Points::new_f64(used.iter().flat_map(|&u| grid_point(ds, u)).collect()),
    };
    Some(DataSet::UnstructuredGrid(UnstructuredGrid { points, cells: out, point_data: pd.gather(&used), cell_data: cd.gather(&cells), field_data: fd.clone() }))
}

pub fn slice_plane(ds: &DataSet, origin: [f64; 3], normal: [f64; 3]) -> Result<PolyData> {
    let n = normalize(normal);
    let plane = |p: [f64; 3]| (p[0] - origin[0]) * n[0] + (p[1] - origin[1]) * n[1] + (p[2] - origin[2]) * n[2];
    // The GPU finds the cells near the plane of a big image (a few more than needed).
    let candidates = match (crate::gpu::kernels(), ds) {
        (Some(g), DataSet::ImageData(img)) if img.dims().iter().all(|&d| d >= 2) && ds.n_cells() >= crate::gpu::MIN_ITEMS => {
            let b = img.bounds();
            let size = (0..3).map(|a| b.min[a].abs().max(b.max[a].abs()) + origin[a].abs()).fold(0.0, f64::max);
            g.image_plane_candidates(img.extent, img.origin, img.spacing, origin, n, 1e-5 * size + 1e-30)
        }
        _ => None,
    };
    let sub = crossing_cells(ds, &|i| u64::from(plane(grid_point(ds, i)) >= 0.0), candidates);
    let ds = match &sub {
        Some(s) => std::borrow::Cow::Borrowed(s),
        None => crate::higher_order::as_linear(ds),
    };
    let (points, ..) = parts(&ds).ok_or_else(|| anyhow::anyhow!("slice: unsupported dataset"))?;
    cut_at_zero(&ds, &ScalarFn::Plane { origin, normal: n, points }, true)
}

pub fn slice_sphere(ds: &DataSet, center: [f64; 3], radius: f64) -> Result<PolyData> {
    let sphere = |p: [f64; 3]| ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2) + (p[2] - center[2]).powi(2)).sqrt() - radius;
    let sub = crossing_cells(ds, &|i| u64::from(sphere(grid_point(ds, i)) >= 0.0), None);
    let ds = match &sub {
        Some(s) => std::borrow::Cow::Borrowed(s),
        None => crate::higher_order::as_linear(ds),
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Slicing a grid through the cells it crosses gives exactly the slice of the whole grid.
    #[test]
    fn grid_slices_match_the_whole_grid() {
        let src = crate::find("wavelet").unwrap();
        let img = src.run(None, &src.default_params()).unwrap();
        let ug = DataSet::UnstructuredGrid(img.to_unstructured().unwrap());
        for (o, n) in [([0.3, -1.2, 0.7], [0.2, 0.5, 1.0]), ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])] {
            let (a, b) = (slice_plane(&img, o, n).unwrap(), slice_plane(&ug, o, n).unwrap());
            assert_eq!(a.polys.connectivity, b.polys.connectivity);
            assert_eq!(a.points.len(), b.points.len());
            assert!((0..a.points.len()).all(|i| a.points.get(i) == b.points.get(i)));
            assert_eq!(a.point_data.get("RTData").unwrap().data.to_f64_vec(), b.point_data.get("RTData").unwrap().data.to_f64_vec());
        }
        let (a, b) = (slice_sphere(&img, [1.0, 2.0, 0.0], 6.0).unwrap(), slice_sphere(&ug, [1.0, 2.0, 0.0], 6.0).unwrap());
        assert_eq!(a.polys.connectivity, b.polys.connectivity);
        for vals in [vec![150.0], vec![100.0, 180.0, 220.0]] {
            let a = crate::contour::contour(&img, crate::Association::Points, "RTData", None, &vals).unwrap();
            let b = crate::contour::contour(&ug, crate::Association::Points, "RTData", None, &vals).unwrap();
            assert_eq!(a.polys.connectivity, b.polys.connectivity, "contour {vals:?}");
            assert!((0..a.points.len()).all(|i| a.points.get(i) == b.points.get(i)), "contour {vals:?} points");
            assert_eq!(a.cell_data.names().collect::<Vec<_>>(), b.cell_data.names().collect::<Vec<_>>());
        }
    }
}
