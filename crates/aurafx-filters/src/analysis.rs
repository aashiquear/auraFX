//! Data analysis filters: histogram, descriptive statistics, integration, gradient.

use crate::params::Association;
use crate::simplex::{as_explicit, decompose, for_each_cell, parts, Simplex};
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;

fn values(ds: &DataSet, assoc: Association, name: &str, comp: Option<usize>) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    let mut found = false;
    let mut visit = |d: &DataSet| {
        let a = match assoc {
            Association::Points => d.point_data(),
            Association::Cells => d.cell_data(),
        }
        .and_then(|a| a.get(name))
        .or_else(|| if let DataSet::Table(t) = d { t.column(name) } else { None });
        if let Some(a) = a {
            found = true;
            for i in 0..a.len() {
                out.push(match comp {
                    Some(c) if c < a.ncomp => a.get(i, c),
                    _ => a.magnitude(i),
                });
            }
        }
    };
    match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().for_each(|(_, d)| visit(d)),
        d => visit(d),
    }
    if !found {
        return Err(anyhow!("no array named {name:?}"));
    }
    Ok(out)
}

/// Histogram as a table: bin_center, bin_start, bin_end, count.
pub fn histogram(ds: &DataSet, assoc: Association, name: &str, comp: Option<usize>, bins: usize, range: Option<(f64, f64)>) -> Result<Table> {
    let v: Vec<f64> = values(ds, assoc, name, comp)?.into_iter().filter(|x| x.is_finite()).collect();
    let bins = bins.max(1);
    let (lo, hi) = range.unwrap_or_else(|| (v.iter().cloned().fold(f64::INFINITY, f64::min), v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)));
    let (lo, hi) = if lo.is_finite() && hi > lo { (lo, hi) } else { (lo.min(0.0), lo.max(0.0) + 1.0) };
    let w = (hi - lo) / bins as f64;
    let mut counts = vec![0.0; bins];
    for x in v {
        if x < lo || x > hi {
            continue;
        }
        let b = (((x - lo) / w) as usize).min(bins - 1);
        counts[b] += 1.0;
    }
    let mut t = Table::default();
    t.columns.insert(Array::scalars_f64("bin_center", (0..bins).map(|i| lo + (i as f64 + 0.5) * w).collect()));
    t.columns.insert(Array::scalars_f64("bin_start", (0..bins).map(|i| lo + i as f64 * w).collect()));
    t.columns.insert(Array::scalars_f64("bin_end", (0..bins).map(|i| lo + (i + 1) as f64 * w).collect()));
    t.columns.insert(Array::scalars_f64(format!("{name}_count"), counts));
    Ok(t)
}

/// Statistics of every numeric array: a table with one row per array/component.
pub fn descriptive_statistics(ds: &DataSet) -> Result<Table> {
    let mut names = Vec::new();
    let mut cols: [Vec<f64>; 7] = Default::default();
    let mut add = |label: String, v: Vec<f64>| {
        let mut v: Vec<f64> = v.into_iter().filter(|x| x.is_finite()).collect();
        if v.is_empty() {
            return;
        }
        let n = v.len() as f64;
        let mean = v.iter().sum::<f64>() / n;
        let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = if v.len() % 2 == 1 { v[v.len() / 2] } else { 0.5 * (v[v.len() / 2 - 1] + v[v.len() / 2]) };
        names.push(label);
        for (k, x) in [n, v[0], v[v.len() - 1], mean, var.sqrt(), median, v.iter().sum::<f64>()].into_iter().enumerate() {
            cols[k].push(x);
        }
    };
    let leaf = match ds {
        DataSet::MultiBlock(mb) => mb.leaves().into_iter().next().map(|(_, d)| d.clone()).ok_or_else(|| anyhow!("empty dataset"))?,
        d => d.clone(),
    };
    for (assoc, prefix) in [(Association::Points, "point"), (Association::Cells, "cell")] {
        let attrs = match assoc {
            Association::Points => leaf.point_data().cloned().or_else(|| if let DataSet::Table(t) = &leaf { Some(t.columns.clone()) } else { None }),
            Association::Cells => leaf.cell_data().cloned(),
        };
        let Some(attrs) = attrs else { continue };
        for a in attrs.iter().filter(|a| a.scalar_type() != ScalarType::String) {
            if a.ncomp == 1 {
                add(format!("{prefix}: {}", a.name), values(ds, assoc, &a.name, Some(0)).unwrap_or_default());
            } else {
                add(format!("{prefix}: {} (magnitude)", a.name), values(ds, assoc, &a.name, None).unwrap_or_default());
                for c in 0..a.ncomp.min(9) {
                    add(format!("{prefix}: {}[{c}]", a.name), values(ds, assoc, &a.name, Some(c)).unwrap_or_default());
                }
            }
        }
    }
    let mut t = Table::default();
    t.columns.insert(Array::new("Variable", 1, ArrayData::String(names)));
    for (k, n) in ["Count", "Minimum", "Maximum", "Mean", "StdDev", "Median", "Sum"].iter().enumerate() {
        t.columns.insert(Array::scalars_f64(*n, std::mem::take(&mut cols[k])));
    }
    Ok(t)
}

fn tet_volume(p: [[f64; 3]; 4]) -> f64 {
    let a = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let b = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
    let c = [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]];
    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])).abs() / 6.0
}

fn tri_area(p: [[f64; 3]; 3]) -> f64 {
    let a = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let b = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
    let c = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
}

/// Integrate point and cell arrays over the highest-dimension cells (volume, area or length).
/// Returns a one-row table with the measure and the integral of every array.
pub fn integrate_variables(ds: &DataSet) -> Result<Table> {
    let mut sums: indexmap::IndexMap<String, f64> = indexmap::IndexMap::new();
    let mut measure = 0.0;
    let mut dim_seen = 0u8;
    let mut visit = |d: &DataSet| -> Result<()> {
        use rayon::prelude::*;
        let d = crate::higher_order::as_linear(d);
        let Some((pts, pd, cd, _)) = parts(&d) else { return Ok(()) };
        let max_dim = match &*d {
            DataSet::UnstructuredGrid(u) => u.cells.types.par_iter().map(|t| t.dimension()).max().unwrap_or(0),
            _ => {
                let mut m = 0u8;
                for_each_cell(&d, |_, ty, _, _| m = m.max(ty.dimension()));
                m
            }
        };
        dim_seen = dim_seen.max(max_dim);
        let parr: Vec<&Array> = pd.iter().filter(|a| a.ncomp == 1 && a.scalar_type() != ScalarType::String).collect();
        let carr: Vec<&Array> = cd.iter().filter(|a| a.ncomp == 1 && a.scalar_type() != ScalarType::String).collect();
        let (np, nc) = (parr.len(), carr.len());
        // Per chunk: [measure, point integrals..., cell integrals...], added in chunk order.
        let chunks = crate::interp::par_cell_chunks(
            &d,
            || vec![0.0f64; 1 + np + nc],
            |acc, cid, ty, ids, faces| {
                if ty.dimension() != max_dim {
                    return;
                }
                decompose(ty, ids, faces, |s| {
                    let mut vs = [0u32; 4];
                    let (m, k) = match s {
                        Simplex::Tet(t) => {
                            vs = t;
                            (tet_volume(t.map(|i| pts.get(i as usize))), 4)
                        }
                        Simplex::Tri(t) => {
                            vs[..3].copy_from_slice(&t);
                            (tri_area(t.map(|i| pts.get(i as usize))), 3)
                        }
                        Simplex::Line([a, b]) => {
                            vs[..2].copy_from_slice(&[a, b]);
                            let (p, q) = (pts.get(a as usize), pts.get(b as usize));
                            (((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt(), 2)
                        }
                        Simplex::Vert(v) => {
                            vs[0] = v;
                            (1.0, 1)
                        }
                    };
                    acc[0] += m;
                    for (j, a) in parr.iter().enumerate() {
                        let avg = vs[..k].iter().map(|&v| a.get(v as usize, 0)).sum::<f64>() / k as f64;
                        acc[1 + j] += avg * m;
                    }
                    for (j, a) in carr.iter().enumerate() {
                        acc[1 + np + j] += a.get(cid, 0) * m;
                    }
                });
            },
        );
        let mut total = vec![0.0f64; 1 + np + nc];
        for c in chunks {
            for (t, v) in total.iter_mut().zip(c) {
                *t += v;
            }
        }
        measure += total[0];
        for (j, a) in parr.iter().enumerate() {
            *sums.entry(a.name.clone()).or_default() += total[1 + j];
        }
        for (j, a) in carr.iter().enumerate() {
            *sums.entry(format!("{} (cell)", a.name)).or_default() += total[1 + np + j];
        }
        Ok(())
    };
    match ds {
        DataSet::MultiBlock(mb) => {
            for (_, d) in mb.leaves() {
                visit(d)?;
            }
        }
        d => visit(d)?,
    }
    let mname = match dim_seen {
        3 => "Volume",
        2 => "Area",
        1 => "Length",
        _ => "Count",
    };
    let mut t = Table::default();
    t.columns.insert(Array::scalars_f64(mname, vec![measure]));
    for (k, v) in sums {
        t.columns.insert(Array::scalars_f64(k, vec![v]));
    }
    Ok(t)
}

/// Gradient of a point scalar (per-cell from linear tets/triangles, averaged to points).
/// Adds `<name>_gradient` (3 components) to point data.
pub fn gradient(ds: &DataSet, name: &str) -> Result<DataSet> {
    let d = as_explicit(ds);
    let (pts, pd, _, _) = parts(&d).ok_or_else(|| anyhow!("gradient: unsupported dataset"))?;
    let f = pd.get(name).ok_or_else(|| anyhow!("gradient: no point array {name:?}"))?;
    let n = pts.len();
    let solve3 = |m: [[f64; 3]; 3], r: [f64; 3]| -> Option<[f64; 3]> {
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if det.abs() < 1e-300 {
            return None;
        }
        let rep = |c: usize| {
            let mut mm = m;
            for row in 0..3 {
                mm[row][c] = r[row];
            }
            mm[0][0] * (mm[1][1] * mm[2][2] - mm[1][2] * mm[2][1]) - mm[0][1] * (mm[1][0] * mm[2][2] - mm[1][2] * mm[2][0])
                + mm[0][2] * (mm[1][0] * mm[2][1] - mm[1][1] * mm[2][0])
        };
        Some([rep(0) / det, rep(1) / det, rep(2) / det])
    };
    // Per chunk: weighted gradient sums of the points it touches (in first-touch order);
    // chunks are added in order, so the result does not depend on the number of threads.
    type Part = (crate::interp::HashMap<u32, usize>, Vec<(u32, [f64; 4])>);
    let add = |st: &mut Part, i: u32, g: [f64; 3], w: f64| {
        let (map, list) = st;
        let k = *map.entry(i).or_insert_with(|| {
            list.push((i, [0.0; 4]));
            list.len() - 1
        });
        let e = &mut list[k].1;
        for c in 0..3 {
            e[c] += g[c] * w;
        }
        e[3] += w;
    };
    let chunks = crate::interp::par_cell_chunks(&d, Part::default, |st, _, ty, ids, faces| {
        decompose(ty, ids, faces, |s| match s {
            Simplex::Tet(t) => {
                let p = t.map(|i| pts.get(i as usize));
                let v = t.map(|i| f.get(i as usize, 0));
                let m = [
                    [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]],
                    [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]],
                    [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]],
                ];
                if let Some(g) = solve3(m, [v[1] - v[0], v[2] - v[0], v[3] - v[0]]) {
                    let vol = tet_volume(p);
                    for &i in &t {
                        add(st, i, g, vol);
                    }
                }
            }
            Simplex::Tri(t) => {
                let p = t.map(|i| pts.get(i as usize));
                let v = t.map(|i| f.get(i as usize, 0));
                let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
                let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
                let nrm = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                // Third equation: gradient has no component along the normal.
                if let Some(g) = solve3([e1, e2, nrm], [v[1] - v[0], v[2] - v[0], 0.0]) {
                    let area = tri_area(p);
                    for &i in &t {
                        add(st, i, g, area);
                    }
                }
            }
            _ => {}
        });
    });
    let mut acc = vec![[0.0f64; 4]; n];
    for (_, list) in chunks {
        for (i, v) in list {
            let a = &mut acc[i as usize];
            for c in 0..4 {
                a[c] += v[c];
            }
        }
    }
    let mut out = Vec::with_capacity(3 * n);
    for a in &acc {
        let w = a[3].max(1e-300);
        out.extend_from_slice(&[a[0] / w, a[1] / w, a[2] / w]);
    }
    // The result keeps the input's type (image data stays image data).
    let mut res = ds.clone();
    match res.point_data_mut() {
        Some(p) => {
            p.insert(Array::new(format!("{name}_gradient"), 3, ArrayData::F64(out)));
        }
        None => bail!("gradient: unsupported dataset"),
    }
    Ok(res)
}
