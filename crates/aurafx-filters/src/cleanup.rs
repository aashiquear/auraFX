//! Clean-up of voxel microstructures and EBSD maps before or after segmentation: turning the
//! sample (and the grid with it), moving and scaling the grid, finding the sample among the
//! scanned cells, eroding and dilating bad data, smoothing feature boundaries by coordination
//! number, and replacing low-confidence cells by neighbours (by confidence, or by how many
//! neighbours agree in orientation). Every cell array moves with the cells it belongs to.

use crate::features::{image, miso, Orients, Vox};
use anyhow::{anyhow, bail, Result};
use aurafx_data::*;

fn cell_f64(img: &ImageData, name: &str) -> Result<Vec<f64>> {
    let a = img.cell_data.get(name).ok_or_else(|| anyhow!("no cell array {name}"))?;
    Ok((0..a.len()).map(|k| a.get(k, 0)).collect())
}

/// Copy every cell array from cell `src[c]` to cell `c`.
fn gather_all(img: &mut ImageData, src: &[usize]) {
    let names: Vec<String> = img.cell_data.iter().map(|a| a.name.clone()).collect();
    for name in names {
        let g = img.cell_data.get(&name).unwrap().gather(src);
        img.cell_data.insert(g);
    }
}

/// Rotation matrix (row-major) for a turn of `deg` degrees about `axis`.
fn rotation(axis: [f64; 3], deg: f64) -> [[f64; 3]; 3] {
    let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if l == 0.0 || deg == 0.0 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let (x, y, z) = (axis[0] / l, axis[1] / l, axis[2] / l);
    let (s, c) = deg.to_radians().sin_cos();
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
}

fn mat_vec(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

/// Transform the grid of a volume: the cells move by `m` (a rotation and scale about the origin)
/// then by `t`; the result is a new axis-aligned grid around the moved sample, each new cell
/// taking every cell array from the old cell under its centre (nearest), cells outside the sample
/// getting zeros. Rotations by multiples of 90 degrees are exact (cells are permuted).
fn transform_grid(img: &ImageData, m: [[f64; 3]; 3], t: [f64; 3]) -> Result<ImageData> {
    let v = Vox::of(img);
    let n = v.n;
    // The new grid: the old cell size turned (90-degree turns keep sizes), around the moved bounds.
    let lo = v.corner;
    let hi = [lo[0] + n[0] as f64 * v.sp[0], lo[1] + n[1] as f64 * v.sp[1], lo[2] + n[2] as f64 * v.sp[2]];
    let mut nlo = [f64::INFINITY; 3];
    let mut nhi = [f64::NEG_INFINITY; 3];
    for c in 0..8 {
        let p = [if c & 1 == 0 { lo[0] } else { hi[0] }, if c & 2 == 0 { lo[1] } else { hi[1] }, if c & 4 == 0 { lo[2] } else { hi[2] }];
        let q = mat_vec(&m, p);
        for a in 0..3 {
            nlo[a] = nlo[a].min(q[a] + t[a]);
            nhi[a] = nhi[a].max(q[a] + t[a]);
        }
    }
    // Spacing: the length of each new axis's preimage step.
    let mut nsp = [0.0; 3];
    for a in 0..3 {
        // The old axis that maps most onto new axis a sets its size.
        let col: Vec<f64> = (0..3).map(|b| (m[a][b] * v.sp[b]).abs()).collect();
        let s = col.iter().copied().fold(0.0, f64::max);
        nsp[a] = if s > 0.0 { s } else { 1.0 };
    }
    let nn: [usize; 3] = std::array::from_fn(|a| (((nhi[a] - nlo[a]) / nsp[a]) - 1e-9).ceil().max(1.0) as usize);
    // Inverse of m.
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-300 {
        bail!("the transformation flattens the sample");
    }
    let inv = [
        [(m[1][1] * m[2][2] - m[1][2] * m[2][1]) / det, (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / det, (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / det],
        [(m[1][2] * m[2][0] - m[1][0] * m[2][2]) / det, (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / det, (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / det],
        [(m[1][0] * m[2][1] - m[1][1] * m[2][0]) / det, (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / det, (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / det],
    ];
    let total = nn[0] * nn[1] * nn[2];
    let src: Vec<Option<usize>> = (0..total)
        .map(|c| {
            let ijk = [c % nn[0], (c / nn[0]) % nn[1], c / (nn[0] * nn[1])];
            let p: [f64; 3] = std::array::from_fn(|a| nlo[a] + (ijk[a] as f64 + 0.5) * nsp[a] - t[a]);
            let q = mat_vec(&inv, p);
            let mut o = [0usize; 3];
            for a in 0..3 {
                let f = (q[a] - lo[a]) / v.sp[a];
                if f < -1e-9 || f >= n[a] as f64 + 1e-9 {
                    return None;
                }
                o[a] = (f.floor() as i64).clamp(0, n[a] as i64 - 1) as usize;
            }
            Some(o[0] + n[0] * (o[1] + n[1] * o[2]))
        })
        .collect();
    let mut out = ImageData {
        extent: [0, nn[0] as i64, 0, nn[1] as i64, 0, nn[2] as i64],
        origin: nlo,
        spacing: nsp,
        field_data: img.field_data.clone(),
        ..Default::default()
    };
    for a in img.cell_data.iter() {
        let idx: Vec<usize> = src.iter().map(|s| s.unwrap_or(0)).collect();
        let mut g = a.gather(&idx);
        // Outside: zeros.
        if let ArrayData::String(_) = g.data {
        } else {
            let mut vals = g.data.to_f64_vec();
            for (c, s) in src.iter().enumerate() {
                if s.is_none() {
                    for k in 0..a.ncomp {
                        vals[c * a.ncomp + k] = 0.0;
                    }
                }
            }
            g.data = match &a.data {
                ArrayData::I32(_) => ArrayData::I32(vals.iter().map(|x| *x as i32).collect()),
                ArrayData::U8(_) => ArrayData::U8(vals.iter().map(|x| *x as u8).collect()),
                ArrayData::F32(_) => ArrayData::F32(vals.iter().map(|x| *x as f32).collect()),
                ArrayData::I64(_) => ArrayData::I64(vals.iter().map(|x| *x as i64).collect()),
                _ => ArrayData::F64(vals),
            };
        }
        out.cell_data.insert(g);
    }
    Ok(out)
}

/// Rotate Sample Reference Frame: turn the sample (its grid and data, not the crystal
/// orientations) by `angle` degrees about `axis`.
pub fn rotate_sample(ds: &DataSet, axis: [f64; 3], angle: f64) -> Result<DataSet> {
    let img = image(ds)?;
    Ok(DataSet::ImageData(transform_grid(img, rotation(axis, angle), [0.0; 3])?))
}

/// Apply Transformation to Geometry: scale, then turn, then move the grid. Without a turn the
/// cells are not resampled (only the origin and spacing change).
pub fn apply_transformation(ds: &DataSet, translation: [f64; 3], axis: [f64; 3], angle: f64, scale: [f64; 3]) -> Result<DataSet> {
    let img = image(ds)?;
    if angle == 0.0 {
        let mut out = img.clone();
        for a in 0..3 {
            out.spacing[a] *= scale[a];
            out.origin[a] = out.origin[a] * scale[a] + translation[a];
        }
        return Ok(DataSet::ImageData(out));
    }
    let r = rotation(axis, angle);
    let m: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| r[i][j] * scale[j]));
    Ok(DataSet::ImageData(transform_grid(img, m, translation)?))
}

/// Apply a 3 x 3 matrix (turn and scale) and a translation to the grid: a diagonal matrix only
/// rescales and moves it; anything else resamples the cells onto a grid around the moved sample.
pub fn apply_matrix(ds: &DataSet, m: [[f64; 3]; 3], t: [f64; 3]) -> Result<DataSet> {
    let img = image(ds)?;
    let diagonal = (0..3).all(|i| (0..3).all(|j| i == j || m[i][j] == 0.0)) && (0..3).all(|i| m[i][i] > 0.0);
    if diagonal {
        let mut out = img.clone();
        for a in 0..3 {
            out.spacing[a] *= m[a][a];
            out.origin[a] = out.origin[a] * m[a][a] + t[a];
        }
        return Ok(DataSet::ImageData(out));
    }
    Ok(DataSet::ImageData(transform_grid(img, m, t)?))
}

/// Identify Sample: the largest face-connected region of good cells (mask true) is the sample;
/// every other good cell becomes bad. With `fill_holes`, bad regions enclosed by the sample
/// (not touching the edge of the scan) become good.
pub fn identify_sample(ds: &DataSet, mask: &str, fill_holes: bool) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let good: Vec<bool> = cell_f64(img, mask)?.iter().map(|x| *x != 0.0).collect();
    let regions = |want: bool| -> Vec<Vec<usize>> {
        let mut seen = vec![false; good.len()];
        let mut out = Vec::new();
        for s in 0..good.len() {
            if good[s] != want || seen[s] {
                continue;
            }
            let mut reg = vec![s];
            seen[s] = true;
            let mut k = 0;
            while k < reg.len() {
                for nb in v.nbrs(reg[k]).into_iter().flatten() {
                    if good[nb] == want && !seen[nb] {
                        seen[nb] = true;
                        reg.push(nb);
                    }
                }
                k += 1;
            }
            out.push(reg);
        }
        out
    };
    let mut keep = vec![false; good.len()];
    if let Some(big) = regions(true).into_iter().max_by_key(|r| r.len()) {
        for c in big {
            keep[c] = true;
        }
    }
    if fill_holes {
        for r in regions(false) {
            if !r.iter().any(|&c| v.on_boundary(c)) {
                for c in r {
                    keep[c] = true;
                }
            }
        }
    }
    let mut out = img.clone();
    let a = out.cell_data.get(mask).unwrap();
    let data = match &a.data {
        ArrayData::U8(_) => ArrayData::U8(keep.iter().map(|&k| k as u8).collect()),
        ArrayData::I32(_) => ArrayData::I32(keep.iter().map(|&k| k as i32).collect()),
        _ => ArrayData::F64(keep.iter().map(|&k| k as u8 as f64).collect()),
    };
    out.cell_data.insert(Array::new(mask, 1, data));
    Ok(DataSet::ImageData(out))
}

/// Isolate Largest Feature: keep the feature with the most cells; every other cell gets feature 0.
pub fn isolate_largest_feature(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let ids = cell_f64(img, ids_name)?;
    let mut count = std::collections::HashMap::new();
    for &f in &ids {
        if f > 0.0 {
            *count.entry(f as i64).or_insert(0usize) += 1;
        }
    }
    let best = count.into_iter().max_by_key(|(f, n)| (*n, -f)).map(|(f, _)| f).ok_or_else(|| anyhow!("no features"))?;
    let mut out = img.clone();
    out.cell_data.insert(Array::new(ids_name, 1, ArrayData::I32(ids.iter().map(|&f| if f as i64 == best { 1 } else { 0 }).collect())));
    Ok(DataSet::ImageData(out))
}

/// Erode/Dilate Bad Data: `dilate` grows the bad cells (feature 0) into their neighbours,
/// `erode` grows the features into the bad cells (each bad cell next to a feature takes, with
/// every cell array, the values of the neighbour of the most common feature); `iterations` times,
/// along the axes switched on.
pub fn erode_dilate_bad(ds: &DataSet, ids_name: &str, erode: bool, iterations: usize, axes: [bool; 3]) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let mut out = img.clone();
    let along = |k: usize| match k {
        0 | 5 => axes[2],
        1 | 4 => axes[1],
        _ => axes[0],
    };
    for _ in 0..iterations {
        let ids = cell_f64(&out, ids_name)?;
        let n = ids.len();
        let mut src: Vec<usize> = (0..n).collect();
        for c in 0..n {
            let bad = ids[c] == 0.0;
            if erode && bad {
                // Take the most common neighbouring feature.
                let mut tally: Vec<(i64, usize, usize)> = Vec::new();
                for (k, nb) in v.nbrs(c).into_iter().enumerate() {
                    if let Some(nb) = nb.filter(|_| along(k)) {
                        let f = ids[nb] as i64;
                        if f > 0 {
                            match tally.iter_mut().find(|t| t.0 == f) {
                                Some(t) => t.1 += 1,
                                None => tally.push((f, 1, nb)),
                            }
                        }
                    }
                }
                if let Some(best) = tally.iter().max_by_key(|t| (t.1, -t.0)) {
                    src[c] = best.2;
                }
            } else if !erode && !bad {
                // A feature cell next to a bad cell becomes bad.
                for (k, nb) in v.nbrs(c).into_iter().enumerate() {
                    if let Some(nb) = nb.filter(|_| along(k)) {
                        if ids[nb] == 0.0 {
                            src[c] = nb;
                            break;
                        }
                    }
                }
            }
        }
        gather_all(&mut out, &src);
    }
    Ok(DataSet::ImageData(out))
}

/// Erode/Dilate Coordination Number: a cell with at least `coordination` face neighbours in one
/// other feature joins that feature (taking its values); with `repeat`, until nothing changes.
pub fn coordination_number(ds: &DataSet, ids_name: &str, coordination: usize, repeat: bool) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let mut out = img.clone();
    for _ in 0..if repeat { 1000 } else { 1 } {
        let ids = cell_f64(&out, ids_name)?;
        let n = ids.len();
        let mut src: Vec<usize> = (0..n).collect();
        let mut changed = false;
        for c in 0..n {
            let mut tally: Vec<(i64, usize, usize)> = Vec::new();
            for nb in v.nbrs(c).into_iter().flatten() {
                let f = ids[nb] as i64;
                if f != ids[c] as i64 && f > 0 {
                    match tally.iter_mut().find(|t| t.0 == f) {
                        Some(t) => t.1 += 1,
                        None => tally.push((f, 1, nb)),
                    }
                }
            }
            if let Some(best) = tally.iter().max_by_key(|t| (t.1, -t.0)) {
                if best.1 >= coordination {
                    src[c] = best.2;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
        gather_all(&mut out, &src);
    }
    Ok(DataSet::ImageData(out))
}

/// Replace Element Attributes with Neighbor (neighbour confidence correlation): every cell whose
/// `confidence` is below `min` (or above, with `greater`) takes every cell array of its
/// neighbour with the best confidence, when that neighbour is better; with `repeat`, until no
/// cell changes.
pub fn replace_with_neighbor(ds: &DataSet, confidence: &str, min: f64, greater: bool, repeat: bool) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let mut out = img.clone();
    let bad = |x: f64| if greater { x > min } else { x < min };
    for _ in 0..if repeat { 1000 } else { 1 } {
        let ci = cell_f64(&out, confidence)?;
        let mut src: Vec<usize> = (0..ci.len()).collect();
        let mut changed = false;
        for c in 0..ci.len() {
            if !bad(ci[c]) {
                continue;
            }
            let best = v.nbrs(c).into_iter().flatten().max_by(|a, b| if greater { ci[*b].total_cmp(&ci[*a]) } else { ci[*a].total_cmp(&ci[*b]) });
            if let Some(b) = best {
                let better = if greater { ci[b] < ci[c] } else { ci[b] > ci[c] };
                if better && !bad(ci[b]) {
                    src[c] = b;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
        gather_all(&mut out, &src);
    }
    Ok(DataSet::ImageData(out))
}

/// Neighbor Orientation Correlation: a cell whose `confidence` is below `min_ci` takes the values
/// of the neighbour that most other neighbours agree with in orientation (misorientation below
/// `tolerance` degrees), when at least 6 - level of them agree; levels 6 down to `level`.
pub fn neighbor_orientation_correlation(ds: &DataSet, confidence: &str, min_ci: f64, tolerance: f64, level: usize, structure: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let mut out = img.clone();
    let tol = tolerance.to_radians();
    for lv in (level.clamp(0, 6)..=6).rev() {
        let need = 6 - lv;
        let ci = cell_f64(&out, confidence)?;
        let o = Orients::of(&DataSet::ImageData(out.clone()), None, crate::crystal::structure_or_phases(structure), None)?;
        let mut src: Vec<usize> = (0..ci.len()).collect();
        for c in 0..ci.len() {
            if ci[c] >= min_ci {
                continue;
            }
            let nbs: Vec<usize> = v.nbrs(c).into_iter().flatten().collect();
            let mut best: Option<(usize, usize)> = None;
            for &a in &nbs {
                let Some((qa, la)) = o.get(a) else { continue };
                let agree = nbs.iter().filter(|&&b| b != a).filter(|&&b| o.get(b).is_some_and(|(qb, lb)| lb == la && miso(qa, qb, la) < tol)).count();
                if best.is_none_or(|bb| agree > bb.1) {
                    best = Some((a, agree));
                }
            }
            if let Some((b, agree)) = best {
                if agree >= need && agree > 0 {
                    src[c] = b;
                }
            }
        }
        gather_all(&mut out, &src);
    }
    Ok(DataSet::ImageData(out))
}

/// Bad Data Neighbor Orientation Check: a bad cell (mask false) with at least `neighbors` good
/// neighbours of nearly its own orientation (within `tolerance` degrees) becomes good; repeated
/// down from 6 neighbours.
pub fn bad_data_neighbor_check(ds: &DataSet, mask: &str, tolerance: f64, neighbors: usize, structure: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let o = Orients::of(ds, None, crate::crystal::structure_or_phases(structure), None)?;
    let mut good: Vec<bool> = cell_f64(img, mask)?.iter().map(|x| *x != 0.0).collect();
    let tol = tolerance.to_radians();
    for need in (neighbors.clamp(1, 6)..=6).rev() {
        loop {
            let mut changed = false;
            for c in 0..good.len() {
                if good[c] {
                    continue;
                }
                let Some((qc, lc)) = o.get(c) else { continue };
                let n = v.nbrs(c).into_iter().flatten().filter(|&b| good[b] && o.get(b).is_some_and(|(qb, lb)| lb == lc && miso(qc, qb, lc) < tol)).count();
                if n >= need {
                    good[c] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
    let mut out = img.clone();
    out.cell_data.insert(Array::new(mask, 1, ArrayData::U8(good.iter().map(|&g| g as u8).collect())));
    Ok(DataSet::ImageData(out))
}

/// Resample a rectilinear grid to image data: cells of the smallest spacing found along each axis
/// (or the given one), each taking the cell and point arrays of the rectilinear cell (point) it
/// falls in (nearest).
pub fn rectilinear_to_image(ds: &DataSet, spacing: [f64; 3]) -> Result<DataSet> {
    let DataSet::RectilinearGrid(r) = ds else { bail!("Resample Rectilinear To Image takes a rectilinear grid") };
    let axes = [&r.x, &r.y, &r.z];
    let mut sp = [1.0; 3];
    let mut n = [1usize; 3];
    let mut lo = [0.0; 3];
    for a in 0..3 {
        let c = axes[a];
        lo[a] = c.first().copied().unwrap_or(0.0);
        let len = c.last().copied().unwrap_or(0.0) - lo[a];
        let min_step = c.windows(2).map(|w| (w[1] - w[0]).abs()).filter(|d| *d > 0.0).fold(f64::INFINITY, f64::min);
        sp[a] = if spacing[a] > 0.0 {
            spacing[a]
        } else if min_step.is_finite() {
            min_step
        } else {
            1.0
        };
        n[a] = if c.len() > 1 { ((len / sp[a]) - 1e-9).ceil().max(1.0) as usize } else { 0 };
    }
    let locate = |a: usize, x: f64| -> usize {
        let c = axes[a];
        if c.len() < 2 {
            return 0;
        }
        c.partition_point(|&v| v <= x).saturating_sub(1).min(c.len() - 2)
    };
    let rd = r.dims();
    let cells = [rd[0].saturating_sub(1).max(1), rd[1].saturating_sub(1).max(1), rd[2].saturating_sub(1).max(1)];
    let nc = [n[0].max(1), n[1].max(1), n[2].max(1)];
    let total = nc[0] * nc[1] * nc[2];
    let src: Vec<usize> = (0..total)
        .map(|c| {
            let ijk = [c % nc[0], (c / nc[0]) % nc[1], c / (nc[0] * nc[1])];
            let o: [usize; 3] = std::array::from_fn(|a| locate(a, lo[a] + (ijk[a] as f64 + 0.5) * sp[a]));
            o[0].min(cells[0] - 1) + cells[0] * (o[1].min(cells[1] - 1) + cells[1] * o[2].min(cells[2] - 1))
        })
        .collect();
    let mut out =
        ImageData { extent: [0, n[0] as i64, 0, n[1] as i64, 0, n[2] as i64], origin: lo, spacing: sp, field_data: r.field_data.clone(), ..Default::default() };
    out.cell_data = r.cell_data.gather(&src);
    if !r.point_data.is_empty() {
        let pn = [n[0] + 1, n[1] + 1, n[2] + 1];
        let ptotal = pn[0] * pn[1] * pn[2];
        let psrc: Vec<usize> = (0..ptotal)
            .map(|c| {
                let ijk = [c % pn[0], (c / pn[0]) % pn[1], c / (pn[0] * pn[1])];
                let o: [usize; 3] = std::array::from_fn(|a| {
                    let x = lo[a] + ijk[a] as f64 * sp[a];
                    let c = axes[a];
                    (0..c.len()).min_by(|&i, &j| (c[i] - x).abs().total_cmp(&(c[j] - x).abs())).unwrap_or(0)
                });
                o[0] + rd[0] * (o[1] + rd[1] * o[2])
            })
            .collect();
        out.point_data = r.point_data.gather(&psrc);
    }
    Ok(DataSet::ImageData(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An n x n x 1 map of 4 square grains (ids 1..4) with Euler angles per grain and CI 1.
    fn grains(n: usize) -> ImageData {
        let mut ids = Vec::new();
        let mut eu = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let g = 1 + (i >= n / 2) as i32 + 2 * (j >= n / 2) as i32;
                ids.push(g);
                eu.extend_from_slice(&[0.3 * g as f32, 0.2 * g as f32, 0.1 * g as f32]);
            }
        }
        let mut im = ImageData { extent: [0, n as i64, 0, n as i64, 0, 1], ..Default::default() };
        im.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(ids)));
        im.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(eu)));
        im.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(vec![1; n * n])));
        im.cell_data.insert(Array::new("Confidence", 1, ArrayData::F32(vec![1.0; n * n])));
        im.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![999, 1])));
        im
    }

    fn ids(d: &DataSet) -> Vec<i32> {
        let a = d.cell_data().unwrap().get("FeatureIds").unwrap();
        (0..a.len()).map(|i| a.get(i, 0) as i32).collect()
    }

    #[test]
    fn noisy_map_cleans_to_its_grains() {
        let n = 20;
        let clean = grains(n);
        // Spoil one cell in 17 inside the grains: wrong id and orientation, low confidence.
        let mut noisy = clean.clone();
        let mut bad = Vec::new();
        let mut eu = noisy.cell_data.get("EulerAngles").unwrap().data.to_f64_vec();
        let mut fid: Vec<i32> = ids(&DataSet::ImageData(clean.clone()));
        let mut ci = vec![1.0f32; n * n];
        for c in (5..n * n).step_by(17) {
            let (i, j) = (c % n, c / n);
            if i == 0 || j == 0 || i + 1 == n || j + 1 == n || i == n / 2 || j == n / 2 || i + 1 == n / 2 || j + 1 == n / 2 {
                continue;
            }
            bad.push(c);
            eu[3 * c] = 2.5;
            eu[3 * c + 1] = 1.1;
            fid[c] = 0;
            ci[c] = 0.01;
        }
        noisy.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(eu.iter().map(|x| *x as f32).collect())));
        noisy.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(fid)));
        noisy.cell_data.insert(Array::new("Confidence", 1, ArrayData::F32(ci)));
        let noisy = DataSet::ImageData(noisy);
        assert!(!bad.is_empty());
        // By confidence: every spoilt cell takes a good neighbour's values.
        let a = replace_with_neighbor(&noisy, "Confidence", 0.1, false, true).unwrap();
        assert_eq!(ids(&a), ids(&DataSet::ImageData(clean.clone())));
        // By orientation agreement.
        let b = neighbor_orientation_correlation(&noisy, "Confidence", 0.1, 5.0, 2, "").unwrap();
        assert_eq!(ids(&b), ids(&DataSet::ImageData(clean.clone())));
        // Eroding the bad data (one pass fills single cells).
        let e = erode_dilate_bad(&noisy, "FeatureIds", true, 1, [true; 3]).unwrap();
        assert_eq!(ids(&e), ids(&DataSet::ImageData(clean)));
    }

    #[test]
    fn sample_rotation_and_coordination() {
        // A 4 x 2 x 1 strip turned 90 degrees about z becomes 2 x 4 x 1.
        let mut im = ImageData { extent: [0, 4, 0, 2, 0, 1], ..Default::default() };
        im.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32((1..=8).collect())));
        let DataSet::ImageData(r) = rotate_sample(&DataSet::ImageData(im.clone()), [0.0, 0.0, 1.0], 90.0).unwrap() else { panic!() };
        assert_eq!((r.extent[1], r.extent[3]), (2, 4));
        let f = r.cell_data.get("FeatureIds").unwrap();
        let v: Vec<i32> = (0..8).map(|i| f.get(i, 0) as i32).collect();
        // Old (i, j) goes to (1 - j, i).
        assert_eq!(v, vec![5, 1, 6, 2, 7, 3, 8, 4]);
        // Moving and scaling only changes the grid.
        let DataSet::ImageData(t) = apply_transformation(&DataSet::ImageData(im), [1.0, 2.0, 3.0], [0.0, 0.0, 1.0], 0.0, [2.0, 2.0, 2.0]).unwrap() else {
            panic!()
        };
        assert_eq!((t.origin, t.spacing), ([1.0, 2.0, 3.0], [2.0, 2.0, 2.0]));
        // A lone cell of grain 2 inside grain 1 joins grain 1 (4 neighbours of 1).
        let mut g = grains(10);
        let mut fid = ids(&DataSet::ImageData(g.clone()));
        fid[3 + 10 * 3] = 2;
        g.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(fid)));
        let c = coordination_number(&DataSet::ImageData(g), "FeatureIds", 3, true).unwrap();
        assert_eq!(ids(&c)[3 + 10 * 3], 1);
        // Identify Sample: two good blobs, the larger is kept; a hole inside it filled.
        let mut m = ImageData { extent: [0, 8, 0, 3, 0, 1], ..Default::default() };
        let mask: Vec<u8> = (0..24)
            .map(|c| {
                let (i, j) = (c % 8, c / 8);
                u8::from((i < 5 && !(i == 2 && j == 1)) || i == 7)
            })
            .collect();
        m.cell_data.insert(Array::new("Mask", 1, ArrayData::U8(mask)));
        let s = identify_sample(&DataSet::ImageData(m), "Mask", true).unwrap();
        let a = s.cell_data().unwrap().get("Mask").unwrap();
        assert_eq!((a.get(2 + 8, 0), a.get(7, 0), a.get(0, 0)), (1.0, 0.0, 1.0));
    }

    #[test]
    fn rectilinear_to_uniform() {
        let r = RectilinearGrid {
            extent: [0, 2, 0, 1, 0, 0],
            x: vec![0.0, 1.0, 3.0],
            y: vec![0.0, 1.0],
            z: vec![0.0],
            cell_data: {
                let mut a = Attributes::new();
                a.insert(Array::scalars_f64("v", vec![10.0, 20.0]));
                a
            },
            ..Default::default()
        };
        let DataSet::ImageData(im) = rectilinear_to_image(&DataSet::RectilinearGrid(r), [0.0; 3]).unwrap() else { panic!() };
        assert_eq!((im.extent[1], im.spacing[0]), (3, 1.0));
        let v = im.cell_data.get("v").unwrap();
        assert_eq!((v.get(0, 0), v.get(1, 0), v.get(2, 0)), (10.0, 20.0, 20.0));
    }
}
