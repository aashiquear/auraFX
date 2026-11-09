//! Mesh checks: what is wrong with each cell (Validate Cells) and how many other cells each
//! cell overlaps (Overlapping Cells Detector).

use crate::simplex::{as_explicit, decompose, for_each_cell, parts, Simplex};
use anyhow::{anyhow, Result};
use aurafx_data::*;

/// Validity flags (bits of the `ValidityState` cell array).
pub const WRONG_POINT_COUNT: i64 = 1;
pub const DEGENERATE: i64 = 2;
pub const INVERTED: i64 = 4;
pub const NON_PLANAR: i64 = 8;
pub const NON_CONVEX: i64 = 16;
pub const REPEATED_POINTS: i64 = 32;

const FLAG_NAMES: [(i64, &str); 6] = [
    (WRONG_POINT_COUNT, "WrongNumberOfPoints"),
    (DEGENERATE, "Degenerate"),
    (INVERTED, "Inverted"),
    (NON_PLANAR, "NonPlanar"),
    (NON_CONVEX, "NonConvex"),
    (REPEATED_POINTS, "RepeatedPoints"),
];

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn det(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    dot(a, cross(b, c))
}

/// Corner neighbours of the linear 3-D cells, ordered so that the determinant of the three edge
/// vectors is positive for a cell in the standard (right-handed) numbering.
fn corners(ty: CellType) -> &'static [[usize; 4]] {
    match ty {
        CellType::Tetra => &[[0, 1, 2, 3], [1, 2, 0, 3], [2, 0, 1, 3], [3, 0, 2, 1]],
        CellType::Hexahedron => &[[0, 1, 3, 4], [1, 2, 0, 5], [2, 3, 1, 6], [3, 0, 2, 7], [4, 7, 5, 0], [5, 4, 6, 1], [6, 5, 7, 2], [7, 6, 4, 3]],
        // The base triangle's normal points away from the top.
        CellType::Wedge => &[[0, 2, 1, 3], [1, 0, 2, 4], [2, 1, 0, 5], [3, 4, 5, 0], [4, 5, 3, 1], [5, 3, 4, 2]],
        // The base's normal points to the apex.
        CellType::Pyramid => &[[0, 1, 3, 4], [1, 2, 0, 4], [2, 3, 1, 4], [3, 0, 2, 4]],
        _ => &[],
    }
}

/// The validity flags of one cell.
pub fn cell_flags(points: &Points, ty: CellType, ids: &[u32], size_tol: f64) -> i64 {
    let mut f = 0;
    if let Some(n) = ty.fixed_point_count() {
        if ids.len() != n {
            return WRONG_POINT_COUNT;
        }
    }
    let p: Vec<[f64; 3]> = ids.iter().map(|&i| points.get(i as usize)).collect();
    if p.is_empty() {
        return WRONG_POINT_COUNT;
    }
    let mut lo = p[0];
    let mut hi = p[0];
    for q in &p {
        for a in 0..3 {
            lo[a] = lo[a].min(q[a]);
            hi[a] = hi[a].max(q[a]);
        }
    }
    let size = (dot(sub(hi, lo), sub(hi, lo))).sqrt().max(1e-300);
    let eps = size * size_tol;
    for i in 0..p.len() {
        for j in i + 1..p.len() {
            if ids[i] == ids[j] || dot(sub(p[i], p[j]), sub(p[i], p[j])).sqrt() <= eps {
                f |= REPEATED_POINTS;
            }
        }
    }
    let cs = corners(ty);
    if !cs.is_empty() {
        let mut min_j = f64::INFINITY;
        for c in cs {
            let j = det(sub(p[c[1]], p[c[0]]), sub(p[c[2]], p[c[0]]), sub(p[c[3]], p[c[0]]));
            min_j = min_j.min(j);
        }
        let scale = size * size * size;
        if min_j.abs() <= scale * 1e-12 {
            f |= DEGENERATE;
        } else if min_j < 0.0 {
            f |= INVERTED;
        }
    }
    match ty {
        CellType::Triangle | CellType::Quad | CellType::Polygon => {
            // Newell normal, then distances from the mean plane and the turn at every corner.
            let mut n = [0.0; 3];
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                n[0] += (a[1] - b[1]) * (a[2] + b[2]);
                n[1] += (a[2] - b[2]) * (a[0] + b[0]);
                n[2] += (a[0] - b[0]) * (a[1] + b[1]);
            }
            let area2 = dot(n, n).sqrt();
            if area2 <= eps * eps {
                f |= DEGENERATE;
            } else {
                let n = [n[0] / area2, n[1] / area2, n[2] / area2];
                let c = p.iter().fold([0.0; 3], |s, q| [s[0] + q[0], s[1] + q[1], s[2] + q[2]]).map(|v| v / p.len() as f64);
                if p.iter().any(|q| dot(sub(*q, c), n).abs() > size * 1e-6) {
                    f |= NON_PLANAR;
                }
                if p.len() > 3 {
                    for i in 0..p.len() {
                        let (a, b, c2) = (p[i], p[(i + 1) % p.len()], p[(i + 2) % p.len()]);
                        if dot(cross(sub(b, a), sub(c2, b)), n) < -size * size * 1e-9 {
                            f |= NON_CONVEX;
                        }
                    }
                }
            }
        }
        CellType::Line | CellType::PolyLine => {
            if p.windows(2).all(|w| dot(sub(w[1], w[0]), sub(w[1], w[0])).sqrt() <= eps) {
                f |= DEGENERATE;
            }
        }
        _ => {}
    }
    f
}

/// Validate Cells: `ValidityState` (0 for a valid cell, else the sum of the flags) on every cell,
/// and field data counting the invalid cells and each problem.
pub fn validate_cells(ds: &DataSet, tolerance: f64) -> Result<DataSet> {
    crate::registry::per_leaf(ds, &|leaf| {
        let ex = as_explicit(leaf).into_owned();
        let (points, ..) = parts(&ex).ok_or_else(|| anyhow!("validate cells: unsupported dataset"))?;
        let mut state = vec![0i64; ex.n_cells()];
        for_each_cell(&ex, |cid, ty, ids, _| state[cid] = cell_flags(points, ty, ids, tolerance));
        let mut out = ex.clone();
        let fd = out.field_data_mut_any();
        fd.insert(Array::scalars_f64("InvalidCells", vec![state.iter().filter(|&&s| s != 0).count() as f64]));
        for (bit, name) in FLAG_NAMES {
            fd.insert(Array::scalars_f64(format!("{name}Cells"), vec![state.iter().filter(|&&s| s & bit != 0).count() as f64]));
        }
        out.cell_data_mut().unwrap().insert_active(AttributeKind::Scalars, Array::new("ValidityState", 1, ArrayData::I64(state)));
        Ok(out)
    })
}

/// Overlapping Cells Detector: `NumberOfOverlapsPerCell`, how many other cells share volume (or,
/// for surface cells in one plane, area) with each cell. Cells that only touch do not count.
pub fn overlapping_cells(ds: &DataSet, tolerance: f64) -> Result<DataSet> {
    let ex = as_explicit(ds).into_owned();
    let (points, ..) = parts(&ex).ok_or_else(|| anyhow!("overlapping cells: unsupported dataset"))?;
    let n = ex.n_cells();
    // Each cell's simplices (tetrahedra, or triangles), corner points and bounding box.
    let mut tets: Vec<Vec<[[f64; 3]; 4]>> = vec![Vec::new(); n];
    let mut tris: Vec<Vec<[[f64; 3]; 3]>> = vec![Vec::new(); n];
    let mut corners: Vec<Vec<[f64; 3]>> = vec![Vec::new(); n];
    for_each_cell(&ex, |cid, ty, ids, faces| {
        corners[cid] = ids.iter().map(|&i| points.get(i as usize)).collect();
        decompose(points, ty, ids, faces, |s| match s {
            Simplex::Tet(t) => tets[cid].push(t.map(|i| points.get(i as usize))),
            Simplex::Tri(t) => tris[cid].push(t.map(|i| points.get(i as usize))),
            _ => {}
        });
    });
    let bbox = |c: &[[f64; 3]]| -> ([f64; 3], [f64; 3]) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for q in c {
            for a in 0..3 {
                lo[a] = lo[a].min(q[a]);
                hi[a] = hi[a].max(q[a]);
            }
        }
        (lo, hi)
    };
    let boxes: Vec<([f64; 3], [f64; 3])> = corners.iter().map(|c| bbox(c)).collect();
    let b = ex.bounds();
    let diag = b.diagonal().max(1e-300);
    let eps = 1e-9;
    // How far inside its cell a sample sits (a fraction of the way to the centre): overlaps
    // shallower than this are not seen.
    let pull = tolerance.clamp(0.0, 0.5).max(0.05);
    // Sample points of a cell: its corners pulled slightly inside, and its centre.
    let samples = |c: usize| -> Vec<[f64; 3]> {
        let k = &corners[c];
        if k.is_empty() {
            return Vec::new();
        }
        let ctr = k.iter().fold([0.0; 3], |s, q| [s[0] + q[0], s[1] + q[1], s[2] + q[2]]).map(|v| v / k.len() as f64);
        let mut s: Vec<[f64; 3]> = k.iter().map(|q| [q[0] + pull * (ctr[0] - q[0]), q[1] + pull * (ctr[1] - q[1]), q[2] + pull * (ctr[2] - q[2])]).collect();
        s.push(ctr);
        s
    };
    let inside_tet = |t: &[[f64; 3]; 4], q: [f64; 3]| -> bool {
        let v = det(sub(t[1], t[0]), sub(t[2], t[0]), sub(t[3], t[0]));
        if v.abs() < 1e-300 {
            return false;
        }
        let l1 = det(sub(q, t[0]), sub(t[2], t[0]), sub(t[3], t[0])) / v;
        let l2 = det(sub(t[1], t[0]), sub(q, t[0]), sub(t[3], t[0])) / v;
        let l3 = det(sub(t[1], t[0]), sub(t[2], t[0]), sub(q, t[0])) / v;
        let l0 = 1.0 - l1 - l2 - l3;
        // Closed: a sample on a face inside the cell (where it is cut into tetrahedra) counts.
        // Samples sit inside their own cell, so cells that only touch never hit.
        [l0, l1, l2, l3].iter().all(|&l| l >= -eps)
    };
    let inside_tri = |t: &[[f64; 3]; 3], q: [f64; 3]| -> bool {
        let nrm = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        let a2 = dot(nrm, nrm);
        if a2 < 1e-300 || dot(sub(q, t[0]), nrm).abs() / a2.sqrt() > diag * 1e-9 {
            return false;
        }
        let l1 = dot(cross(sub(q, t[0]), sub(t[2], t[0])), nrm) / a2;
        let l2 = dot(cross(sub(t[1], t[0]), sub(q, t[0])), nrm) / a2;
        let l0 = 1.0 - l1 - l2;
        [l0, l1, l2].iter().all(|&l| l >= -eps)
    };
    let hits = |a: usize, bb: usize| -> bool {
        let s = samples(a);
        if !tets[bb].is_empty() {
            s.iter().any(|&q| tets[bb].iter().any(|t| inside_tet(t, q)))
        } else if !tris[bb].is_empty() && tets[a].is_empty() {
            s.iter().any(|&q| tris[bb].iter().any(|t| inside_tri(t, q)))
        } else {
            false
        }
    };
    // Bins of bounding boxes over a coarse grid.
    let g = ((n as f64).cbrt().ceil() as usize).clamp(1, 64);
    let cell_of = |x: f64, a: usize| (((x - b.min[a]) / (b.max[a] - b.min[a]).max(1e-300) * g as f64) as usize).min(g - 1);
    let mut bins: std::collections::HashMap<[usize; 3], Vec<usize>> = Default::default();
    for (c, (lo, hi)) in boxes.iter().enumerate() {
        if corners[c].is_empty() {
            continue;
        }
        for i in cell_of(lo[0], 0)..=cell_of(hi[0], 0) {
            for j in cell_of(lo[1], 1)..=cell_of(hi[1], 1) {
                for k in cell_of(lo[2], 2)..=cell_of(hi[2], 2) {
                    bins.entry([i, j, k]).or_default().push(c);
                }
            }
        }
    }
    use rayon::prelude::*;
    let count: Vec<i64> = (0..n)
        .into_par_iter()
        .map(|a| {
            if corners[a].is_empty() {
                return 0;
            }
            let (lo, hi) = boxes[a];
            let mut cand: Vec<usize> = Vec::new();
            for i in cell_of(lo[0], 0)..=cell_of(hi[0], 0) {
                for j in cell_of(lo[1], 1)..=cell_of(hi[1], 1) {
                    for k in cell_of(lo[2], 2)..=cell_of(hi[2], 2) {
                        if let Some(v) = bins.get(&[i, j, k]) {
                            cand.extend(v.iter().copied().filter(|&c| c != a));
                        }
                    }
                }
            }
            cand.sort_unstable();
            cand.dedup();
            cand.into_iter()
                .filter(|&c| {
                    let (l2, h2) = boxes[c];
                    (0..3).all(|ax| l2[ax] < hi[ax] && h2[ax] > lo[ax]) && (hits(a, c) || hits(c, a))
                })
                .count() as i64
        })
        .collect();
    let mut out = ex.clone();
    out.field_data_mut_any().insert(Array::scalars_f64("OverlappingCells", vec![count.iter().filter(|&&c| c > 0).count() as f64]));
    out.cell_data_mut().unwrap().insert_active(AttributeKind::Scalars, Array::new("NumberOfOverlapsPerCell", 1, ArrayData::I64(count)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(points: &mut Points, o: [f64; 3]) -> Vec<u32> {
        let base = points.len() as u32;
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]] {
            points.push([p[0] + o[0], p[1] + o[1], p[2] + o[2]]);
        }
        (base..base + 8).collect()
    }

    #[test]
    fn flags() {
        let mut ug = UnstructuredGrid::default();
        let h = hex(&mut ug.points, [0.0; 3]);
        ug.cells.push(CellType::Hexahedron, &h);
        // The same hexahedron with top and bottom swapped: inverted.
        let mut inv = h[4..].to_vec();
        inv.extend_from_slice(&h[..4]);
        ug.cells.push(CellType::Hexahedron, &inv);
        // A tetrahedron flattened onto a plane: degenerate.
        ug.cells.push(CellType::Tetra, &[h[0], h[1], h[2], h[3]]);
        // A dart (non-convex) and a bent quad (non-planar).
        let k = ug.points.len() as u32;
        ug.points.push([0.3, 0.3, 0.0]);
        ug.cells.push(CellType::Quad, &[h[0], h[1], k, h[3]]);
        ug.cells.push(CellType::Quad, &[h[0], h[1], h[6], h[3]]);
        // A wedge in the standard numbering, and a correct pyramid.
        ug.cells.push(CellType::Wedge, &[h[0], h[3], h[1], h[4], h[7], h[5]]);
        ug.cells.push(CellType::Pyramid, &[h[0], h[1], h[2], h[3], h[6]]);
        let out = validate_cells(&DataSet::UnstructuredGrid(ug), 1e-9).unwrap();
        let s = out.cell_data().unwrap().get("ValidityState").unwrap();
        let v: Vec<i64> = (0..7).map(|i| s.get(i, 0) as i64).collect();
        assert_eq!(v[0], 0);
        assert_eq!(v[1], INVERTED);
        assert_eq!(v[2], DEGENERATE);
        assert!(v[3] & NON_CONVEX != 0, "{v:?}");
        assert!(v[4] & NON_PLANAR != 0, "{v:?}");
        assert_eq!((v[5], v[6]), (0, 0), "{v:?}");
        assert_eq!(out.field_data().get("InvalidCells").unwrap().get(0, 0), 4.0);
    }

    #[test]
    fn overlaps() {
        let mut ug = UnstructuredGrid::default();
        let a = hex(&mut ug.points, [0.0; 3]);
        let b = hex(&mut ug.points, [1.0, 0.0, 0.0]); // touches a: no overlap
        let c = hex(&mut ug.points, [0.5, 0.5, 0.0]); // overlaps a and b
        let d = hex(&mut ug.points, [5.0, 5.0, 5.0]); // alone
        for h in [&a, &b, &c, &d] {
            ug.cells.push(CellType::Hexahedron, h);
        }
        let out = overlapping_cells(&DataSet::UnstructuredGrid(ug), 0.0).unwrap();
        let n = out.cell_data().unwrap().get("NumberOfOverlapsPerCell").unwrap();
        assert_eq!((0..4).map(|i| n.get(i, 0) as i64).collect::<Vec<_>>(), vec![1, 1, 2, 0]);
    }
}
