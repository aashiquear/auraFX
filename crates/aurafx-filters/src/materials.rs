//! Volume fractions: the interface of each material inside mixed cells, as a plane per cell
//! placed so that the cut leaves the material's volume fraction on its side (Youngs' method,
//! the plane facing down the fraction's gradient), and fragments: connected regions of cells
//! above a fraction, with their volumes, centres and integrated arrays.

use crate::simplex::{as_explicit, decompose, for_each_cell, parts, Simplex};
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;
use std::collections::HashMap;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn tet_vol(p: [[f64; 3]; 4]) -> f64 {
    dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))).abs() / 6.0
}
fn tri_area(p: [[f64; 3]; 3]) -> f64 {
    let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    0.5 * dot(c, c).sqrt()
}
fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])]
}

/// Volume of tetrahedron `p` where n·x - d <= 0.
pub fn tet_below(p: [[f64; 3]; 4], n: [f64; 3], d: f64) -> f64 {
    let s: Vec<f64> = p.iter().map(|q| dot(*q, n) - d).collect();
    let below: Vec<usize> = (0..4).filter(|&i| s[i] <= 0.0).collect();
    let above: Vec<usize> = (0..4).filter(|&i| s[i] > 0.0).collect();
    let v = tet_vol(p);
    let cut = |i: usize, j: usize| lerp(p[i], p[j], s[i] / (s[i] - s[j]));
    match below.len() {
        0 => 0.0,
        4 => v,
        1 => {
            let i = below[0];
            above.iter().map(|&j| s[i] / (s[i] - s[j])).product::<f64>() * v
        }
        3 => {
            let j = above[0];
            v - below.iter().map(|&i| s[j] / (s[j] - s[i])).product::<f64>() * v
        }
        _ => {
            // A prism: (b0, c00, c01) and (b1, c10, c11).
            let (b0, b1, a0, a1) = (below[0], below[1], above[0], above[1]);
            let (x0, x1, x2) = (p[b0], cut(b0, a0), cut(b0, a1));
            let (y0, y1, y2) = (p[b1], cut(b1, a0), cut(b1, a1));
            tet_vol([x0, x1, x2, y0]) + tet_vol([x1, x2, y0, y1]) + tet_vol([x2, y0, y1, y2])
        }
    }
}

/// Area of triangle `p` where n·x - d <= 0.
pub fn tri_below(p: [[f64; 3]; 3], n: [f64; 3], d: f64) -> f64 {
    let s: Vec<f64> = p.iter().map(|q| dot(*q, n) - d).collect();
    let below: Vec<usize> = (0..3).filter(|&i| s[i] <= 0.0).collect();
    let above: Vec<usize> = (0..3).filter(|&i| s[i] > 0.0).collect();
    let a = tri_area(p);
    match below.len() {
        0 => 0.0,
        3 => a,
        1 => {
            let i = below[0];
            above.iter().map(|&j| s[i] / (s[i] - s[j])).product::<f64>() * a
        }
        _ => {
            let j = above[0];
            a - below.iter().map(|&i| s[j] / (s[j] - s[i])).product::<f64>() * a
        }
    }
}

/// A cell as its simplices: tetrahedra for volumes, triangles for surface cells.
struct CellGeom {
    tets: Vec<[[f64; 3]; 4]>,
    tris: Vec<[[f64; 3]; 3]>,
    corners: Vec<[f64; 3]>,
    edges: Vec<([f64; 3], [f64; 3])>,
}

impl CellGeom {
    fn measure(&self) -> f64 {
        if !self.tets.is_empty() {
            self.tets.iter().map(|t| tet_vol(*t)).sum()
        } else {
            self.tris.iter().map(|t| tri_area(*t)).sum()
        }
    }
    fn below(&self, n: [f64; 3], d: f64) -> f64 {
        if !self.tets.is_empty() {
            self.tets.iter().map(|t| tet_below(*t, n, d)).sum()
        } else {
            self.tris.iter().map(|t| tri_below(*t, n, d)).sum()
        }
    }
}

fn geometry(ex: &DataSet) -> Result<(Vec<CellGeom>, Vec<Vec<u32>>)> {
    let (points, ..) = parts(ex).ok_or_else(|| anyhow!("unsupported data"))?;
    let mut cells = Vec::with_capacity(ex.n_cells());
    let mut ids = Vec::with_capacity(ex.n_cells());
    for_each_cell(ex, |_, ty, pts, faces| {
        let mut g = CellGeom { tets: Vec::new(), tris: Vec::new(), corners: pts.iter().map(|&i| points.get(i as usize)).collect(), edges: Vec::new() };
        decompose(points, ty, pts, faces, |s| match s {
            Simplex::Tet(t) => g.tets.push(t.map(|i| points.get(i as usize))),
            Simplex::Tri(t) => g.tris.push(t.map(|i| points.get(i as usize))),
            _ => {}
        });
        let es = ty.edges();
        if !es.is_empty() {
            g.edges = es.iter().filter(|e| e[0] < pts.len() && e[1] < pts.len()).map(|e| (g.corners[e[0]], g.corners[e[1]])).collect();
        } else if g.corners.len() > 2 {
            g.edges = (0..g.corners.len()).map(|k| (g.corners[k], g.corners[(k + 1) % g.corners.len()])).collect();
        }
        cells.push(g);
        ids.push(pts.to_vec());
    });
    Ok((cells, ids))
}

/// Material Interface: for each volume-fraction cell array (in order), the interface in every
/// mixed cell, one block of polygons (lines for surface meshes) per material, with cell data
/// `VolumeFraction`, and field data `MaterialVolume` (the sum of fraction times cell volume).
pub fn youngs(ds: &DataSet, arrays: &[String], eps: f64) -> Result<DataSet> {
    let ex = as_explicit(ds).into_owned();
    let (points, _, cd, _) = parts(&ex).ok_or_else(|| anyhow!("material interface: unsupported data (use a mesh, image or grid)"))?;
    let names: Vec<String> = if arrays.is_empty() {
        cd.iter().filter(|a| a.ncomp == 1 && a.scalar_type() != ScalarType::String && !a.name.starts_with("vtk")).map(|a| a.name.clone()).collect()
    } else {
        arrays.to_vec()
    };
    if names.is_empty() {
        bail!("no volume-fraction cell arrays");
    }
    let (cells, ids) = geometry(&ex)?;
    let np = points.len();
    let mut point_cells: Vec<Vec<usize>> = vec![Vec::new(); np];
    for (c, pts) in ids.iter().enumerate() {
        for &p in pts {
            point_cells[p as usize].push(c);
        }
    }
    let mut mb = MultiBlock::default();
    for name in &names {
        let fa = cd.get(name).ok_or_else(|| anyhow!("no cell array named {name:?}"))?;
        let f: Vec<f64> = (0..cells.len()).map(|c| fa.get(c, 0).clamp(0.0, 1.0)).collect();
        // Fractions at the points (averages of the cells around), for the gradient.
        let pf: Vec<f64> = point_cells.iter().map(|cs| if cs.is_empty() { 0.0 } else { cs.iter().map(|&c| f[c]).sum::<f64>() / cs.len() as f64 }).collect();
        let mut out = PolyData::default();
        let mut vf = Vec::new();
        let mut total = 0.0;
        for (c, g) in cells.iter().enumerate() {
            let vol = g.measure();
            total += f[c] * vol;
            if f[c] <= eps || f[c] >= 1.0 - eps || vol <= 0.0 {
                continue;
            }
            // Least-squares gradient of the point fractions over the cell's corners.
            let k = &g.corners;
            let ctr = k.iter().fold([0.0; 3], |s, q| [s[0] + q[0], s[1] + q[1], s[2] + q[2]]).map(|v| v / k.len() as f64);
            let fm = ids[c].iter().map(|&p| pf[p as usize]).sum::<f64>() / k.len() as f64;
            let mut m = [[0.0; 3]; 3];
            let mut r = [0.0; 3];
            for (q, &p) in k.iter().zip(&ids[c]) {
                let d = sub(*q, ctr);
                for i in 0..3 {
                    for j in 0..3 {
                        m[i][j] += d[i] * d[j];
                    }
                    r[i] += d[i] * (pf[p as usize] - fm);
                }
            }
            let scale = (m[0][0] + m[1][1] + m[2][2]).max(1e-300);
            for (i, row) in m.iter_mut().enumerate() {
                row[i] += 1e-9 * scale;
            }
            let grad = solve3(m, r);
            let gl = dot(grad, grad).sqrt();
            if gl <= 1e-300 {
                continue;
            }
            // The material lies down the gradient's opposite: n·x <= d holds the material.
            let n = [-grad[0] / gl, -grad[1] / gl, -grad[2] / gl];
            let (mut lo, mut hi) = k.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(dot(*q, n)), b.max(dot(*q, n))));
            let target = f[c] * vol;
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if g.below(n, mid) < target {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let d = 0.5 * (lo + hi);
            // The plane across the cell's edges.
            let mut cut: Vec<[f64; 3]> = Vec::new();
            for &(a, b) in &g.edges {
                let (sa, sb) = (dot(a, n) - d, dot(b, n) - d);
                if (sa <= 0.0) != (sb <= 0.0) {
                    let p = lerp(a, b, sa / (sa - sb));
                    if !cut.iter().any(|q| dot(sub(*q, p), sub(*q, p)) < 1e-24 * scale) {
                        cut.push(p);
                    }
                }
            }
            if cut.len() < 2 {
                continue;
            }
            let base = out.points.len() as u32;
            if cut.len() == 2 || g.tets.is_empty() {
                cut.truncate(2);
                for p in &cut {
                    out.points.push(*p);
                }
                out.lines.push(CellType::Line, &[base, base + 1]);
            } else {
                // Order the points around their centre in the plane.
                let pc = cut.iter().fold([0.0; 3], |s, q| [s[0] + q[0], s[1] + q[1], s[2] + q[2]]).map(|v| v / cut.len() as f64);
                let u = {
                    let t = sub(cut[0], pc);
                    let l = dot(t, t).sqrt().max(1e-300);
                    t.map(|x| x / l)
                };
                let w = cross(n, u);
                cut.sort_by(|a, b| {
                    let (da, db) = (sub(*a, pc), sub(*b, pc));
                    dot(da, w).atan2(dot(da, u)).total_cmp(&dot(db, w).atan2(dot(db, u)))
                });
                for p in &cut {
                    out.points.push(*p);
                }
                out.polys.push(CellType::Polygon, &(base..base + cut.len() as u32).collect::<Vec<_>>());
            }
            vf.push(f[c]);
        }
        out.cell_data.insert(Array::scalars_f64("VolumeFraction", vf));
        out.field_data.insert(Array::scalars_f64("MaterialVolume", vec![total]));
        mb.blocks.push((name.clone(), Some(DataSet::PolyData(out))));
    }
    Ok(DataSet::MultiBlock(mb))
}

fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> [f64; 3] {
    let det = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-300 {
        return [0.0; 3];
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut a = m;
        for i in 0..3 {
            a[i][k] = r[i];
        }
        *o = det(a) / d;
    }
    out
}

/// Fragments: cells whose `array` is at least `threshold`, joined into connected regions
/// (through shared faces; for AMR levels, through touching boxes). Returns a table with one row
/// per fragment (FragmentId, CellCount, Volume = sum of fraction x cell volume, Centroid, and
/// the volume integral of every other cell array), or the cells with `FragmentId`.
pub fn fragments(ds: &DataSet, array: &str, threshold: f64, cells_out: bool) -> Result<DataSet> {
    // The visible cells of every leaf, as boxes (structured levels) or explicit cells.
    let leaves: Vec<&DataSet> = match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().map(|l| l.1).collect(),
        d => vec![d],
    };
    struct FCell {
        leaf: usize,
        cell: usize,
        frac: f64,
        vol: f64,
        ctr: [f64; 3],
        lo: [f64; 3],
        hi: [f64; 3],
        pts: Vec<[f64; 3]>,
    }
    let mut fc: Vec<FCell> = Vec::new();
    let mut exs: Vec<DataSet> = Vec::new();
    let mut all_images = true;
    for (li, leaf) in leaves.iter().enumerate() {
        let Some(fa) = leaf.cell_data().and_then(|c| c.get(array)) else { continue };
        let ghost = leaf.cell_data().and_then(|c| c.get(aurafx_data::amr::GHOST));
        all_images &= matches!(leaf, DataSet::ImageData(_));
        let ex = as_explicit(leaf).into_owned();
        let (geo, _) = geometry(&ex)?;
        for (c, g) in geo.iter().enumerate() {
            let hidden = ghost.is_some_and(|gh| (gh.get(c, 0) as u8) & aurafx_data::amr::MASKED != 0);
            let f = fa.get(c, 0);
            if hidden || !(f >= threshold) {
                continue;
            }
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for q in &g.corners {
                for a in 0..3 {
                    lo[a] = lo[a].min(q[a]);
                    hi[a] = hi[a].max(q[a]);
                }
            }
            let ctr = g.corners.iter().fold([0.0; 3], |s, q| [s[0] + q[0], s[1] + q[1], s[2] + q[2]]).map(|v| v / g.corners.len().max(1) as f64);
            fc.push(FCell { leaf: li, cell: c, frac: f.min(1.0), vol: g.measure(), ctr, lo, hi, pts: g.corners.clone() });
        }
        exs.push(ex);
    }
    if fc.is_empty() {
        bail!("no cells with {array} at or above {threshold}");
    }
    // Union-find over adjacency.
    let mut parent: Vec<usize> = (0..fc.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let b = ds.bounds();
    let tol = (b.diagonal() * 1e-9).max(1e-300);
    if all_images {
        // Boxes that touch over an area (not just along an edge or at a corner).
        let size = fc.iter().map(|c| (0..3).map(|a| c.hi[a] - c.lo[a]).filter(|s| *s > 0.0).fold(f64::INFINITY, f64::min)).fold(f64::INFINITY, f64::min);
        let size = if size.is_finite() && size > 0.0 { size } else { 1.0 };
        let key = |x: f64, a: usize| ((x - b.min[a]) / size).floor() as i64;
        let mut bins: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, c) in fc.iter().enumerate() {
            for x in key(c.lo[0] - tol, 0)..=key(c.hi[0] + tol, 0) {
                for y in key(c.lo[1] - tol, 1)..=key(c.hi[1] + tol, 1) {
                    for z in key(c.lo[2] - tol, 2)..=key(c.hi[2] + tol, 2) {
                        bins.entry([x, y, z]).or_default().push(i);
                    }
                }
            }
        }
        for v in bins.values() {
            for (k, &i) in v.iter().enumerate() {
                for &j in &v[k + 1..] {
                    let (a, c) = (&fc[i], &fc[j]);
                    let mut touch = 0;
                    let mut ok = true;
                    for ax in 0..3 {
                        let ov = a.hi[ax].min(c.hi[ax]) - a.lo[ax].max(c.lo[ax]);
                        let flat = a.hi[ax] - a.lo[ax] <= tol && c.hi[ax] - c.lo[ax] <= tol;
                        if flat {
                            continue;
                        }
                        if ov < -tol {
                            ok = false;
                        } else if ov <= tol {
                            touch += 1;
                        }
                    }
                    if ok && touch <= 1 {
                        let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                        parent[ri] = rj;
                    }
                }
            }
        }
    } else {
        // Cells sharing a face: at least three corners (two for surface cells) in common.
        let mut by_point: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, c) in fc.iter().enumerate() {
            for q in &c.pts {
                by_point.entry([(q[0] / tol).round() as i64, (q[1] / tol).round() as i64, (q[2] / tol).round() as i64]).or_default().push(i);
            }
        }
        let mut shared: HashMap<(usize, usize), usize> = HashMap::new();
        for v in by_point.values() {
            for (k, &i) in v.iter().enumerate() {
                for &j in &v[k + 1..] {
                    if i != j {
                        *shared.entry((i.min(j), i.max(j))).or_default() += 1;
                    }
                }
            }
        }
        for ((i, j), n) in shared {
            let need = if fc[i].pts.len() >= 4 && fc[j].pts.len() >= 4 { 3 } else { 2 };
            if n >= need {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    // Number the fragments by their first cell.
    let mut id_of: HashMap<usize, usize> = HashMap::new();
    let mut frag = vec![0usize; fc.len()];
    for i in 0..fc.len() {
        let r = find(&mut parent, i);
        let next = id_of.len();
        frag[i] = *id_of.entry(r).or_insert(next);
    }
    let nf = id_of.len();
    if cells_out {
        // The fragment cells of every leaf, with FragmentId.
        let mut mb = MultiBlock::default();
        for (li, ex) in exs.iter().enumerate() {
            let keep: Vec<usize> = fc.iter().filter(|c| c.leaf == li).map(|c| c.cell).collect();
            if keep.is_empty() {
                continue;
            }
            let ids: Vec<f64> = fc.iter().zip(&frag).filter(|(c, _)| c.leaf == li).map(|(_, &f)| f as f64).collect();
            let mut u = crate::partitions::subset(&ex.to_unstructured().ok_or_else(|| anyhow!("unsupported data"))?, &keep);
            u.cell_data.insert(Array::scalars_f64("FragmentId", ids));
            mb.blocks.push((format!("fragments {li}"), Some(DataSet::UnstructuredGrid(u))));
        }
        return Ok(if mb.blocks.len() == 1 { mb.blocks.pop().unwrap().1.unwrap() } else { DataSet::MultiBlock(mb) });
    }
    let mut count = vec![0.0; nf];
    let mut vol = vec![0.0; nf];
    let mut ctr = vec![[0.0; 3]; nf];
    // Volume integrals of the other cell arrays (one component).
    let others: Vec<String> = exs
        .first()
        .and_then(|e| e.cell_data())
        .map(|cd| {
            cd.iter()
                .filter(|a| a.ncomp == 1 && a.name != array && !a.name.starts_with("vtk") && a.scalar_type() != ScalarType::String)
                .map(|a| a.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let mut integ = vec![vec![0.0; nf]; others.len()];
    for (c, &f) in fc.iter().zip(&frag) {
        let w = c.frac * c.vol;
        count[f] += 1.0;
        vol[f] += w;
        for a in 0..3 {
            ctr[f][a] += w * c.ctr[a];
        }
        let cd = leaves[c.leaf].cell_data();
        for (k, n) in others.iter().enumerate() {
            if let Some(arr) = cd.and_then(|cd| cd.get(n)) {
                integ[k][f] += c.vol * arr.get(c.cell, 0);
            }
        }
    }
    let mut t = Table::default();
    t.columns.insert(Array::new("FragmentId", 1, ArrayData::I64((0..nf as i64).collect())));
    t.columns.insert(Array::scalars_f64("CellCount", count));
    t.columns.insert(Array::scalars_f64("Volume", vol.clone()));
    t.columns.insert(Array::new(
        "Centroid",
        3,
        ArrayData::F64(ctr.iter().zip(&vol).flat_map(|(c, v)| c.map(|x| if *v > 0.0 { x / v } else { 0.0 })).collect()),
    ));
    for (k, n) in others.iter().enumerate() {
        t.columns.insert(Array::scalars_f64(format!("Integral {n}"), std::mem::take(&mut integ[k])));
    }
    t.field_data.insert(Array::scalars_f64("FragmentCount", vec![nf as f64]));
    Ok(DataSet::Table(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid over [-1, 1]^3 with cell arrays of the fraction of each cell inside two balls.
    fn balls(n: usize, centres: &[([f64; 3], f64)]) -> DataSet {
        let h = 2.0 / n as f64;
        let mut im = ImageData { extent: [0, n as i64, 0, n as i64, 0, n as i64], origin: [-1.0; 3], spacing: [h; 3], ..Default::default() };
        let s = 6;
        let mut f = Vec::with_capacity(n * n * n);
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    let mut inside = 0;
                    for (a, b, c) in itertools3(s) {
                        let p = [
                            -1.0 + (i as f64 + (a as f64 + 0.5) / s as f64) * h,
                            -1.0 + (j as f64 + (b as f64 + 0.5) / s as f64) * h,
                            -1.0 + (k as f64 + (c as f64 + 0.5) / s as f64) * h,
                        ];
                        if centres.iter().any(|(c0, r)| dot(sub(p, *c0), sub(p, *c0)) <= r * r) {
                            inside += 1;
                        }
                    }
                    f.push(inside as f64 / (s * s * s) as f64);
                }
            }
        }
        im.cell_data.insert(Array::scalars_f64("frac", f));
        DataSet::ImageData(im)
    }

    fn itertools3(s: usize) -> impl Iterator<Item = (usize, usize, usize)> {
        (0..s).flat_map(move |a| (0..s).flat_map(move |b| (0..s).map(move |c| (a, b, c))))
    }

    #[test]
    fn clipped_volumes() {
        let t = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        // x <= 0.5 holds 1 - 0.5^3 of the unit tetrahedron.
        assert!((tet_below(t, [1.0, 0.0, 0.0], 0.5) - (1.0 - 0.125) / 6.0).abs() < 1e-14);
        // x + y <= 0.5: two corners on each side.
        let v: f64 = tet_below(t, [1.0, 1.0, 0.0], 0.5);
        // The integral of 1 - x - y over x + y <= 1/2 in the first quadrant: 1/12.
        assert!((v - 1.0 / 12.0).abs() < 1e-14, "{v}");
        let tri = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        assert!((tri_below(tri, [1.0, 0.0, 0.0], 0.5) - 0.375).abs() < 1e-14);
    }

    #[test]
    fn interface_of_a_ball() {
        let r = 0.6;
        let ds = balls(24, &[([0.0; 3], r)]);
        let DataSet::MultiBlock(mb) = youngs(&ds, &[], 1e-6).unwrap() else { panic!() };
        let Some(DataSet::PolyData(p)) = &mb.blocks[0].1 else { panic!() };
        // Area within a few percent of the sphere's, points near radius r.
        let mut area = 0.0;
        for (_, c) in p.polys.iter() {
            let q: Vec<[f64; 3]> = c.iter().map(|&i| p.points.get(i as usize)).collect();
            for k in 1..q.len() - 1 {
                area += tri_area([q[0], q[k], q[k + 1]]);
            }
        }
        let sphere = 4.0 * std::f64::consts::PI * r * r;
        assert!((area / sphere - 1.0).abs() < 0.04, "{area} {sphere}");
        let h = 2.0 / 24.0;
        for i in 0..p.points.len() {
            let q = p.points.get(i);
            assert!((dot(q, q).sqrt() - r).abs() < 0.35 * h, "{q:?}");
        }
        let v = mb.blocks[0].1.as_ref().unwrap().field_data().get("MaterialVolume").unwrap().get(0, 0);
        assert!((v / (4.0 / 3.0 * std::f64::consts::PI * r * r * r) - 1.0).abs() < 0.01, "{v}");
    }

    #[test]
    fn fragments_of_two_balls() {
        let (r1, r2) = (0.35, 0.25);
        let ds = balls(32, &[([-0.45, 0.0, 0.0], r1), ([0.5, 0.2, 0.0], r2)]);
        let DataSet::Table(t) = fragments(&ds, "frac", 1e-9, false).unwrap() else { panic!() };
        assert_eq!(t.n_rows(), 2);
        let mut vols: Vec<f64> = (0..2).map(|i| t.columns.get("Volume").unwrap().get(i, 0)).collect();
        vols.sort_by(|a, b| a.total_cmp(b));
        let exact = |r: f64| 4.0 / 3.0 * std::f64::consts::PI * r * r * r;
        assert!((vols[0] / exact(r2) - 1.0).abs() < 0.01 && (vols[1] / exact(r1) - 1.0).abs() < 0.01, "{vols:?}");
        let cells = fragments(&ds, "frac", 0.5, true).unwrap();
        assert!(cells.cell_data().unwrap().get("FragmentId").is_some());
    }
}
