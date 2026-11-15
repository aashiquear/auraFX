//! Measures of features (grains) and their boundaries: neighbourhoods, largest cross sections,
//! clustering (radial distribution of feature centres), boundary strength and slip transmission
//! (m', F1, F1spt, F7 for a loading direction), Schmid factors on twin boundaries, c axes of
//! hexagonal crystals (per cell, and against the feature's average) and micro-texture regions
//! (neighbouring features whose c axes line up).

use crate::features::{average_quats, image, Orients, Vox};
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::mechanics::{slip_systems, SlipSystem};
use aurafx_crystal::rotation::{conj, rotate};
use aurafx_crystal::{Laue, Quat};
use aurafx_data::*;

fn ids_of(img: &ImageData, name: &str) -> Result<Vec<i32>> {
    let a = img.cell_data.get(name).ok_or_else(|| anyhow!("no cell array {name} (run Segment Features first)"))?;
    Ok((0..a.len()).map(|k| a.get(k, 0) as i32).collect())
}

fn n_features(ids: &[i32]) -> usize {
    ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Cell count, centroid and equivalent sphere (circle) diameter of every feature.
fn sizes(v: &Vox, ids: &[i32], nf: usize) -> (Vec<usize>, Vec<[f64; 3]>, Vec<f64>) {
    let mut n = vec![0usize; nf];
    let mut c = vec![[0.0; 3]; nf];
    for (cell, &f) in ids.iter().enumerate() {
        if f > 0 {
            let p = v.center(cell);
            n[f as usize] += 1;
            for a in 0..3 {
                c[f as usize][a] += p[a];
            }
        }
    }
    let flat = v.n.iter().filter(|&&k| k == 1).count() >= 1;
    let cell_v = v.sp[0] * v.sp[1] * if flat { 1.0 } else { v.sp[2] };
    let esd: Vec<f64> = n
        .iter()
        .map(|&k| {
            let vol = k as f64 * cell_v;
            if flat {
                2.0 * (vol / std::f64::consts::PI).sqrt()
            } else {
                (6.0 * vol / std::f64::consts::PI).cbrt()
            }
        })
        .collect();
    for f in 0..nf {
        if n[f] > 0 {
            c[f] = c[f].map(|x| x / n[f] as f64);
        }
    }
    (n, c, esd)
}

/// Find Neighborhoods: for every feature, how many other features have their centre within
/// `multiples` times the mean equivalent diameter of its centre (`Neighborhoods`).
pub fn neighborhoods(ds: &DataSet, ids_name: &str, multiples: f64) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let nf = n_features(&ids);
    let (n, c, esd) = sizes(&v, &ids, nf);
    let present: Vec<usize> = (1..nf).filter(|&f| n[f] > 0).collect();
    let mean = present.iter().map(|&f| esd[f]).sum::<f64>() / present.len().max(1) as f64;
    let r = multiples * mean;
    let pts: Vec<[f64; 3]> = present.iter().map(|&f| c[f]).collect();
    let tree = aurafx_data::kdtree::KdTree::new(&pts);
    let mut out = vec![0.0; nf];
    for (k, &f) in present.iter().enumerate() {
        out[f] = (tree.within(pts[k], r).len() - 1) as f64;
    }
    let mut res = img.clone();
    res.field_data.insert(Array::scalars_f64("Neighborhoods", out));
    Ok(DataSet::ImageData(res))
}

/// Find Largest Cross Sections: the largest area a feature covers in one slice normal to `axis`
/// (0 x, 1 y, 2 z) (`LargestCrossSections`).
pub fn largest_cross_sections(ds: &DataSet, ids_name: &str, axis: usize) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let nf = n_features(&ids);
    let a = axis.min(2);
    let (u, w) = match a {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let area = v.sp[u] * v.sp[w];
    let mut per_slice = vec![vec![0usize; nf]; v.n[a]];
    for (cell, &f) in ids.iter().enumerate() {
        if f > 0 {
            per_slice[v.ijk(cell)[a]][f as usize] += 1;
        }
    }
    let out: Vec<f64> = (0..nf).map(|f| per_slice.iter().map(|s| s[f]).max().unwrap_or(0) as f64 * area).collect();
    let mut res = img.clone();
    res.field_data.insert(Array::scalars_f64("LargestCrossSections", out));
    Ok(DataSet::ImageData(res))
}

/// Find Feature Clustering: the radial distribution function of feature centres (features of
/// `phase`, 0 = all): pairs at each distance over the pairs a random arrangement of the same
/// density would give. A table of r and g(r), with the edge effect left in (centres near the
/// sample's edge have fewer partners).
pub fn clustering(ds: &DataSet, ids_name: &str, bins: usize, max_distance: f64, phase: i32) -> Result<Table> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let nf = n_features(&ids);
    let (n, c, _) = sizes(&v, &ids, nf);
    let phases = img.cell_data.get("Phases");
    let mut fphase = vec![0i32; nf];
    if let Some(p) = phases {
        for (cell, &f) in ids.iter().enumerate() {
            if f > 0 {
                fphase[f as usize] = p.get(cell, 0) as i32;
            }
        }
    }
    let pts: Vec<[f64; 3]> = (1..nf).filter(|&f| n[f] > 0 && (phase == 0 || fphase[f] == phase)).map(|f| c[f]).collect();
    if pts.len() < 2 {
        bail!("fewer than two features to measure");
    }
    let size: [f64; 3] = std::array::from_fn(|a| v.n[a] as f64 * v.sp[a]);
    let flat = v.n.iter().any(|&k| k == 1);
    let volume = if flat {
        size[0] * size[1] * size[2].max(1e-300) / size[2].max(1e-300) * if v.n[2] == 1 { 1.0 } else { size[2] }
    } else {
        size[0] * size[1] * size[2]
    };
    let rmax = if max_distance > 0.0 { max_distance } else { 0.5 * size.iter().copied().filter(|s| *s > 0.0).fold(f64::INFINITY, f64::min) };
    let bins = bins.max(1);
    let dr = rmax / bins as f64;
    let mut count = vec![0.0; bins];
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            let d = dot(sub(pts[i], pts[j]), sub(pts[i], pts[j])).sqrt();
            if d < rmax {
                count[(d / dr) as usize] += 2.0;
            }
        }
    }
    let np = pts.len() as f64;
    let density = np / volume;
    let g: Vec<f64> = (0..bins)
        .map(|b| {
            let (r0, r1) = (b as f64 * dr, (b + 1) as f64 * dr);
            let shell = if flat { std::f64::consts::PI * (r1 * r1 - r0 * r0) } else { 4.0 / 3.0 * std::f64::consts::PI * (r1.powi(3) - r0.powi(3)) };
            count[b] / (np * density * shell)
        })
        .collect();
    let mut t = Table::default();
    t.columns.insert(Array::scalars_f64("r", (0..bins).map(|b| (b as f64 + 0.5) * dr).collect()));
    t.columns.insert(Array::scalars_f64("RDF", g));
    t.field_data.insert(Array::scalars_f64("Features", vec![np]));
    Ok(t)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Neighbouring feature pairs and how many cell faces they share.
fn pairs(v: &Vox, ids: &[i32]) -> std::collections::BTreeMap<(i32, i32), usize> {
    let mut m = std::collections::BTreeMap::new();
    for c in 0..ids.len() {
        for nb in v.nbrs(c).into_iter().flatten().filter(|&nb| nb > c) {
            let (a, b) = (ids[c], ids[nb]);
            if a > 0 && b > 0 && a != b {
                *m.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
    }
    m
}

/// The slip systems of a crystal class (FCC {111}<110>, BCC {110}<111>, HCP basal and prismatic <a>).
fn systems_for(laue: Laue, lattice: &str) -> Vec<SlipSystem> {
    match (laue, lattice) {
        (_, "BCC") => slip_systems(&[1], 1.633),
        (Laue::HexagonalHigh | Laue::HexagonalLow, _) | (_, "HCP") => slip_systems(&[4, 5], 1.587),
        _ => slip_systems(&[0], 1.0),
    }
}

/// Boundary strength and slip transmission across every boundary between two features, for a
/// load along `d` (sample frame): with each grain's slip systems in the sample frame,
/// m' = |cos psi cos kappa| (psi between the plane normals, kappa between the directions) of the
/// two systems with the highest Schmid factors (`mPrime`), the largest m' of any pair (`F1`),
/// the largest m' times both systems' Schmid factors over the largest Schmid factors (`F1spt`)
/// and the largest |cos kappa| (`F7`, direction alignment). A table with a row per pair.
pub fn boundary_strengths(ds: &DataSet, ids_name: &str, d: [f64; 3], lattice: &str, structure: &str) -> Result<Table> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let nf = n_features(&ids);
    let o = Orients::of(ds, None, crate::crystal::structure_or_phases(structure), None)?;
    let avg = average_quats(&o, &ids, nf);
    let laue: Vec<Option<Laue>> = {
        let mut l = vec![None; nf];
        for (c, &f) in ids.iter().enumerate() {
            if f > 0 && l[f as usize].is_none() {
                l[f as usize] = o.get(c).map(|x| x.1);
            }
        }
        l
    };
    let dl = dot(d, d).sqrt().max(1e-300);
    let d = d.map(|x| x / dl);
    // Each grain's systems in the sample frame with their Schmid factors.
    let sample = |q: Quat, sys: &[SlipSystem]| -> Vec<([f64; 3], [f64; 3], f64)> {
        let qi = conj(q);
        sys.iter()
            .map(|s| {
                let (n, sd) = (rotate(qi, s.n), rotate(qi, s.s));
                (n, sd, (dot(d, n) * dot(d, sd)).abs())
            })
            .collect()
    };
    let (mut fa, mut fb, mut faces, mut mp, mut f1, mut f1spt, mut f7) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    for ((a, b), k) in pairs(&v, &ids) {
        let (Some(qa), Some(qb), Some(la), Some(lb)) = (avg[a as usize], avg[b as usize], laue[a as usize], laue[b as usize]) else { continue };
        let sa = sample(qa, &systems_for(la, lattice));
        let sb = sample(qb, &systems_for(lb, lattice));
        let ma = sa.iter().map(|x| x.2).fold(0.0, f64::max).max(1e-300);
        let mb = sb.iter().map(|x| x.2).fold(0.0, f64::max).max(1e-300);
        let top = |s: &[([f64; 3], [f64; 3], f64)]| s.iter().max_by(|x, y| x.2.total_cmp(&y.2)).cloned().unwrap();
        let (ta, tb) = (top(&sa), top(&sb));
        let mprime = |x: &([f64; 3], [f64; 3], f64), y: &([f64; 3], [f64; 3], f64)| (dot(x.0, y.0) * dot(x.1, y.1)).abs();
        let mut best_f1 = 0.0f64;
        let mut best_spt = 0.0f64;
        let mut best_f7 = 0.0f64;
        for x in &sa {
            for y in &sb {
                let m = mprime(x, y);
                best_f1 = best_f1.max(m);
                best_spt = best_spt.max(m * (x.2 / ma) * (y.2 / mb));
                best_f7 = best_f7.max(dot(x.1, y.1).abs());
            }
        }
        fa.push(a as f64);
        fb.push(b as f64);
        faces.push(k as f64);
        mp.push(mprime(&ta, &tb));
        f1.push(best_f1);
        f1spt.push(best_spt);
        f7.push(best_f7);
    }
    let mut t = Table::default();
    for (n, col) in [("FeatureA", fa), ("FeatureB", fb), ("SharedFaces", faces), ("mPrime", mp), ("F1", f1), ("F1spt", f1spt), ("F7", f7)] {
        t.columns.insert(Array::scalars_f64(n, col));
    }
    Ok(t)
}

/// Schmid factors on twin boundaries: for every pair of neighbouring cubic features that are
/// twins (60 degrees about a shared <111>, within the tolerances), the Schmid factors for a load
/// along `d` of the three <110> slip directions in the twin plane, in each grain.
pub fn twin_boundary_schmid(ds: &DataSet, ids_name: &str, d: [f64; 3], axis_tol: f64, angle_tol: f64, structure: &str) -> Result<Table> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let nf = n_features(&ids);
    let o = Orients::of(ds, None, crate::crystal::structure_or_phases(structure), None)?;
    let avg = average_quats(&o, &ids, nf);
    let dl = dot(d, d).sqrt().max(1e-300);
    let d = d.map(|x| x / dl);
    let mut cols: Vec<Vec<f64>> = vec![Vec::new(); 8];
    for ((a, b), _) in pairs(&v, &ids) {
        let (Some(qa), Some(qb)) = (avg[a as usize], avg[b as usize]) else { continue };
        if !crate::features::is_twin(qa, qb, axis_tol, angle_tol) {
            continue;
        }
        let (_, axis) = aurafx_crystal::disorientation(qa, qb, Laue::CubicHigh);
        // The <111> closest to the misorientation axis is the twin plane's normal (crystal of a).
        let n = axis.map(|x| if x < 0.0 { -1.0 } else { 1.0 } / 3f64.sqrt());
        let dirs: Vec<[f64; 3]> = [[1.0, -1.0, 0.0], [1.0, 0.0, -1.0], [0.0, 1.0, -1.0], [1.0, 1.0, 0.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]]
            .iter()
            .map(|s| s.map(|x| x / 2f64.sqrt()))
            .filter(|s| dot(*s, n).abs() < 1e-9)
            .collect();
        cols[0].push(a as f64);
        cols[1].push(b as f64);
        // Grain a: its own frame; grain b: the same plane and directions carried into b's frame.
        let ca = rotate(qa, d);
        let rel = aurafx_crystal::rotation::mul(qb, conj(qa));
        let cb = rotate(qb, d);
        for (k, s) in dirs.iter().take(3).enumerate() {
            cols[2 + k].push((dot(ca, *s) * dot(ca, n)).abs());
            let (sb, nb) = (rotate(rel, *s), rotate(rel, n));
            cols[5 + k].push((dot(cb, sb) * dot(cb, nb)).abs());
        }
    }
    let mut t = Table::default();
    let names = ["FeatureA", "FeatureB", "SchmidA1", "SchmidA2", "SchmidA3", "SchmidB1", "SchmidB2", "SchmidB3"];
    for (k, nme) in names.iter().enumerate() {
        t.columns.insert(Array::scalars_f64(*nme, std::mem::take(&mut cols[k])));
    }
    Ok(t)
}

/// The c axis of every hexagonal cell in the sample frame (`CAxisLocation`, unit vectors; zero
/// for other cells).
pub fn c_axis_locations(ds: &DataSet, structure: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let o = Orients::of(ds, None, crate::crystal::structure_or_phases(structure), None)?;
    let n = o.q.len();
    let mut v = vec![0.0; n * 3];
    for c in 0..n {
        if let Some((q, l)) = o.get(c) {
            if matches!(l, Laue::HexagonalHigh | Laue::HexagonalLow) {
                let a = rotate(conj(q), [0.0, 0.0, 1.0]);
                // The upper hemisphere (c and -c are the same axis).
                let s = if a[2] < 0.0 { -1.0 } else { 1.0 };
                for k in 0..3 {
                    v[3 * c + k] = s * a[k];
                }
            }
        }
    }
    let mut out = img.clone();
    out.cell_data.insert(Array::new("CAxisLocation", 3, ArrayData::F64(v)));
    Ok(DataSet::ImageData(out))
}

fn feature_c_axes(c: &[[f64; 3]], ids: &[i32], nf: usize) -> Vec<Option<[f64; 3]>> {
    // The mean axis: the main eigenvector of sum(c c^T) (sign-free).
    let mut m = vec![[[0.0; 3]; 3]; nf];
    let mut any = vec![false; nf];
    for (cell, &f) in ids.iter().enumerate() {
        let a = c[cell];
        if f > 0 && dot(a, a) > 0.0 {
            any[f as usize] = true;
            for i in 0..3 {
                for j in 0..3 {
                    m[f as usize][i][j] += a[i] * a[j];
                }
            }
        }
    }
    (0..nf)
        .map(|f| {
            if !any[f] {
                return None;
            }
            let (vals, vecs) = crate::features::sym_eigen(m[f]);
            let k = (0..3).max_by(|&a, &b| vals[a].total_cmp(&vals[b])).unwrap();
            Some([vecs[0][k], vecs[1][k], vecs[2][k]])
        })
        .collect()
}

fn c_axes_of(ds: &DataSet, structure: &str) -> Result<Vec<[f64; 3]>> {
    let d = c_axis_locations(ds, structure)?;
    let a = d.cell_data().unwrap().get("CAxisLocation").unwrap();
    Ok((0..a.len()).map(|i| [a.get(i, 0), a.get(i, 1), a.get(i, 2)]).collect())
}

/// Feature Reference C-Axis Misorientations: the angle (degrees) between each hexagonal cell's
/// c axis and its feature's mean c axis (`FeatureReferenceCAxisMisorientations`), and the mean
/// per feature (`FeatureAvgCAxisMisorientations`).
pub fn reference_c_axis(ds: &DataSet, ids_name: &str, structure: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let ids = ids_of(img, ids_name)?;
    let nf = n_features(&ids);
    let c = c_axes_of(ds, structure)?;
    let mean = feature_c_axes(&c, &ids, nf);
    let mut cell = vec![0.0; ids.len()];
    let mut sum = vec![0.0; nf];
    let mut cnt = vec![0usize; nf];
    for (k, &f) in ids.iter().enumerate() {
        if f > 0 {
            if let Some(m) = mean[f as usize] {
                if dot(c[k], c[k]) > 0.0 {
                    let a = dot(c[k], m).abs().min(1.0).acos().to_degrees();
                    cell[k] = a;
                    sum[f as usize] += a;
                    cnt[f as usize] += 1;
                }
            }
        }
    }
    let mut out = img.clone();
    out.cell_data.insert(Array::scalars_f64("FeatureReferenceCAxisMisorientations", cell));
    out.field_data
        .insert(Array::scalars_f64("FeatureAvgCAxisMisorientations", (0..nf).map(|f| if cnt[f] > 0 { sum[f] / cnt[f] as f64 } else { 0.0 }).collect()));
    Ok(DataSet::ImageData(out))
}

/// Identify Micro-Texture Regions: neighbouring hexagonal features whose mean c axes are within
/// `tolerance` degrees join one region (`MTRIds` per cell, `ParentIds` per feature); regions of
/// fewer than `min_features` features keep their own number but count as none (`IsMTR` 0).
pub fn micro_texture_regions(ds: &DataSet, ids_name: &str, tolerance: f64, min_features: usize, structure: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = ids_of(img, ids_name)?;
    let nf = n_features(&ids);
    let c = c_axes_of(ds, structure)?;
    let mean = feature_c_axes(&c, &ids, nf);
    let cos_tol = tolerance.to_radians().cos();
    let mut parent: Vec<usize> = (0..nf).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for ((a, b), _) in pairs(&v, &ids) {
        if let (Some(x), Some(y)) = (mean[a as usize], mean[b as usize]) {
            if dot(x, y).abs() >= cos_tol {
                let (ra, rb) = (find(&mut parent, a as usize), find(&mut parent, b as usize));
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    // Number the regions from 1 in feature order.
    let mut number = std::collections::HashMap::new();
    let mut pid = vec![0i32; nf];
    let mut members = std::collections::HashMap::<usize, usize>::new();
    for f in 1..nf {
        let r = find(&mut parent, f);
        *members.entry(r).or_default() += 1;
    }
    for f in 1..nf {
        let r = find(&mut parent, f);
        let next = number.len() as i32 + 1;
        pid[f] = *number.entry(r).or_insert(next);
    }
    let is_mtr: Vec<f64> = (0..nf).map(|f| if f > 0 && members[&find(&mut parent, f)] >= min_features.max(2) { 1.0 } else { 0.0 }).collect();
    let mut out = img.clone();
    out.cell_data.insert(Array::new("MTRIds", 1, ArrayData::I32(ids.iter().map(|&f| if f > 0 { pid[f as usize] } else { 0 }).collect())));
    out.field_data.insert(Array::new("ParentIds", 1, ArrayData::I32(pid)));
    out.field_data.insert(Array::scalars_f64("IsMTR", is_mtr));
    Ok(DataSet::ImageData(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An nx x ny x 1 map split into vertical strips of `w` cells, feature k with Euler angles e(k).
    fn strips(nx: usize, ny: usize, w: usize, eul: impl Fn(usize) -> [f32; 3], hex: bool) -> DataSet {
        let mut ids = Vec::new();
        let mut eu = Vec::new();
        for _j in 0..ny {
            for i in 0..nx {
                let f = i / w + 1;
                ids.push(f as i32);
                eu.extend_from_slice(&eul(f));
            }
        }
        let mut im = ImageData { extent: [0, nx as i64, 0, ny as i64, 0, 1], ..Default::default() };
        im.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(ids)));
        im.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(eu)));
        im.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(vec![1; nx * ny])));
        im.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![999, if hex { 0 } else { 1 }])));
        DataSet::ImageData(im)
    }

    #[test]
    fn sizes_sections_neighborhoods() {
        let ds = strips(12, 4, 3, |_| [0.0; 3], false);
        let l = largest_cross_sections(&ds, "FeatureIds", 0).unwrap();
        // A slice normal to x through a strip: 4 cells of area 1.
        assert_eq!(l.field_data().get("LargestCrossSections").unwrap().get(1, 0), 4.0);
        let n = neighborhoods(&ds, "FeatureIds", 1.0).unwrap();
        // Strips 3 apart, mean ESD 2 sqrt(12 / pi) = 3.9: the neighbours on each side.
        let nb = n.field_data().get("Neighborhoods").unwrap();
        assert_eq!((nb.get(1, 0), nb.get(2, 0)), (1.0, 2.0));
    }

    #[test]
    fn boundary_metrics_of_identical_and_twin_grains() {
        // Two identical grains: m' = 1 for the same systems, F7 = 1.
        let same = strips(8, 4, 4, |_| [0.3, 0.4, 0.5], false);
        let t = boundary_strengths(&same, "FeatureIds", [0.0, 0.0, 1.0], "", "").unwrap();
        assert_eq!(t.n_rows(), 1);
        for c in ["mPrime", "F1", "F7"] {
            assert!((t.columns.get(c).unwrap().get(0, 0) - 1.0).abs() < 1e-6, "{c}");
        }
        // A twin: 60 degrees about [111].
        let q1 = aurafx_crystal::rotation::euler_to_quat([0.0, 0.0, 0.0]);
        let q2 = aurafx_crystal::rotation::mul(aurafx_crystal::rotation::axis_angle_to_quat([1.0, 1.0, 1.0], 60f64.to_radians()), q1);
        let e2 = aurafx_crystal::rotation::quat_to_euler(q2);
        let twin = strips(8, 4, 4, move |f| if f == 1 { [0.0; 3] } else { [e2[0] as f32, e2[1] as f32, e2[2] as f32] }, false);
        let tt = twin_boundary_schmid(&twin, "FeatureIds", [0.0, 0.0, 1.0], 2.0, 2.0, "").unwrap();
        assert_eq!(tt.n_rows(), 1);
        // Load along [001] of grain a, twin plane (111): Schmid factor of [1-10] is 0, of [10-1] and [01-1] 1/sqrt(6).
        let s: Vec<f64> = (1..=3).map(|k| tt.columns.get(&format!("SchmidA{k}")).unwrap().get(0, 0)).collect();
        let mut s = s;
        s.sort_by(|a, b| a.total_cmp(b));
        assert!(s[0].abs() < 1e-6 && (s[1] - 1.0 / 6f64.sqrt()).abs() < 1e-6 && (s[2] - 1.0 / 6f64.sqrt()).abs() < 1e-6, "{s:?}");
    }

    #[test]
    fn c_axes_and_micro_texture() {
        // Hexagonal strips: 1 and 2 share a c axis tilted 10 degrees from z; 3 is tilted 60 degrees.
        let ds = strips(9, 3, 3, |f| if f < 3 { [0.0, 10f32.to_radians(), 0.0] } else { [0.0, 60f32.to_radians(), 0.0] }, true);
        let c = c_axis_locations(&ds, "").unwrap();
        let a = c.cell_data().unwrap().get("CAxisLocation").unwrap();
        assert!((a.get(0, 2) - 10f64.to_radians().cos()).abs() < 1e-6);
        let r = reference_c_axis(&ds, "FeatureIds", "").unwrap();
        assert!(r.cell_data().unwrap().get("FeatureReferenceCAxisMisorientations").unwrap().get(0, 0).abs() < 1e-3);
        let m = micro_texture_regions(&ds, "FeatureIds", 5.0, 2, "").unwrap();
        let ids = m.cell_data().unwrap().get("MTRIds").unwrap();
        assert_eq!((ids.get(0, 0), ids.get(4, 0), ids.get(8, 0)), (1.0, 1.0, 2.0));
        assert_eq!(m.field_data().get("IsMTR").unwrap().get(3, 0), 0.0);
    }

    #[test]
    fn clustering_of_a_lattice() {
        // Centres on a square lattice of pitch 4: g(r) peaks at r = 4.
        let n = 40;
        let mut ids = Vec::new();
        for j in 0..n {
            for i in 0..n {
                ids.push(1 + (i / 4 + 10 * (j / 4)) as i32);
            }
        }
        let mut im = ImageData { extent: [0, n as i64, 0, n as i64, 0, 1], ..Default::default() };
        im.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(ids)));
        let t = clustering(&DataSet::ImageData(im), "FeatureIds", 20, 10.0, 0).unwrap();
        let g = t.columns.get("RDF").unwrap();
        let r = t.columns.get("r").unwrap();
        let k = (0..20).max_by(|&a, &b| g.get(a, 0).total_cmp(&g.get(b, 0))).unwrap();
        assert!((r.get(k, 0) - 4.0).abs() <= 0.5, "{}", r.get(k, 0));
        assert_eq!(g.get(0, 0), 0.0);
    }
}
