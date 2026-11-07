//! Features (grains) of voxel microstructures: segmentation, clean-up, and per-feature and
//! per-cell statistics.
//!
//! A feature is a connected set of cells (6-connected: cells sharing a face) given one id in
//! the cell array `FeatureIds` (0 = no feature). Per-feature results are field arrays with one
//! row per feature id (row 0 for id 0). Algorithms follow DREAM.3D 6 (BSD-3, BlueQuartz
//! Software) where it defines them; see the user guide for where auraFX differs.

use crate::crystal::{orientation_array, Repr};
use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::{self as cx, Laue, Quat};
use aurafx_data::*;
use rayon::prelude::*;

/// The cell grid of an image: cells along each axis (a flat axis holds one layer), cell size
/// and the corner of cell 0.
#[derive(Clone, Copy, Debug)]
pub struct Vox {
    pub n: [usize; 3],
    pub sp: [f64; 3],
    pub corner: [f64; 3],
    pub flat: [bool; 3],
}

impl Vox {
    pub fn of(img: &ImageData) -> Vox {
        let e = img.extent;
        let flat = [e[1] == e[0], e[3] == e[2], e[5] == e[4]];
        Vox {
            n: [(e[1] - e[0]).max(1) as usize, (e[3] - e[2]).max(1) as usize, (e[5] - e[4]).max(1) as usize],
            sp: img.spacing.map(|s| s.abs()),
            corner: std::array::from_fn(|k| img.origin[k] + e[2 * k] as f64 * img.spacing[k]),
            flat,
        }
    }
    pub fn len(&self) -> usize {
        self.n[0] * self.n[1] * self.n[2]
    }
    pub fn ijk(&self, c: usize) -> [usize; 3] {
        [c % self.n[0], (c / self.n[0]) % self.n[1], c / (self.n[0] * self.n[1])]
    }
    /// Face neighbours in the order -z, -y, -x, +x, +y, +z.
    pub fn nbrs(&self, c: usize) -> [Option<usize>; 6] {
        let [i, j, k] = self.ijk(c);
        let (nx, nxy) = (self.n[0], self.n[0] * self.n[1]);
        [
            (k > 0).then(|| c - nxy),
            (j > 0).then(|| c - nx),
            (i > 0).then(|| c - 1),
            (i + 1 < self.n[0]).then(|| c + 1),
            (j + 1 < self.n[1]).then(|| c + nx),
            (k + 1 < self.n[2]).then(|| c + nxy),
        ]
    }
    pub fn on_boundary(&self, c: usize) -> bool {
        let p = self.ijk(c);
        (0..3).any(|a| !self.flat[a] && self.n[a] > 1 && (p[a] == 0 || p[a] + 1 == self.n[a])) || (0..2).any(|a| self.flat[2] && (p[a] == 0 || p[a] + 1 == self.n[a]))
    }
    /// Centre of cell `c`.
    pub fn center(&self, c: usize) -> [f64; 3] {
        let p = self.ijk(c);
        std::array::from_fn(|a| self.corner[a] + (p[a] as f64 + 0.5) * self.sp[a])
    }
    fn is_2d(&self) -> bool {
        self.flat.iter().any(|f| *f) || self.n.iter().any(|n| *n == 1)
    }
}

pub(crate) fn image(ds: &DataSet) -> Result<&ImageData> {
    match ds {
        DataSet::ImageData(i) => Ok(i),
        _ => bail!("this filter works on image data (voxel volumes and EBSD maps)"),
    }
}

fn cell_i32(img: &ImageData, name: &str) -> Result<Vec<i32>> {
    let a = img.cell_data.get(name).ok_or_else(|| anyhow!("no cell array {name} (run Segment Features first)"))?;
    Ok((0..a.len()).map(|k| a.get(k, 0) as i32).collect())
}

fn mask_of(img: &ImageData, mask: Option<&str>) -> Result<Option<Vec<bool>>> {
    match mask.filter(|m| !m.is_empty()) {
        Some(m) => {
            let a = img.cell_data.get(m).ok_or_else(|| anyhow!("no mask array {m}"))?;
            Ok(Some((0..a.len()).map(|k| a.get(k, 0) != 0.0).collect()))
        }
        None => Ok(None),
    }
}

/// Field arrays that are part of the phase table, never per-feature.
const ENSEMBLE: [&str; 3] = ["CrystalStructures", "LatticeConstants", "MaterialName"];

/// The number of feature rows (max id + 1).
fn rows(ids: &[i32]) -> usize {
    ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1
}

/// Names of the per-feature field arrays of a table with `n` rows.
fn feature_arrays(img: &ImageData, n: usize) -> Vec<String> {
    img.field_data.iter().filter(|a| a.len() == n && !ENSEMBLE.contains(&a.name.as_str())).map(|a| a.name.clone()).collect()
}

/// Per-cell orientation (as an f64 quaternion) and crystal class, None where there is none.
pub struct Orients {
    pub q: Vec<[f32; 4]>,
    /// Laue code per cell; 255 = none.
    pub laue: Vec<u8>,
    pub phase: Vec<i32>,
}

impl Orients {
    pub fn of(ds: &DataSet, orientations: Option<(Association, String)>, structure: &str, phases: Option<&str>) -> Result<Orients> {
        let (assoc, a) = orientation_array(ds, orientations)?;
        if assoc != Association::Cells {
            bail!("the orientations must be cell data");
        }
        let repr = Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
        let cls = crate::crystal::classes_of(ds, assoc, a.len(), structure, phases)?;
        let pname = phases.filter(|p| !p.is_empty()).unwrap_or("Phases");
        let ph = ds.cell_data().and_then(|c| c.get(pname));
        let n = a.len();
        let q: Vec<[f32; 4]> = (0..n).into_par_iter().map(|i| repr.decode_values(a, i).map(|v| v as f32)).collect();
        let laue = cls.iter().map(|l| l.map(|l| l.code() as u8).unwrap_or(255)).collect();
        let phase = (0..n).map(|i| ph.map(|p| p.get(i, 0) as i32).unwrap_or(1)).collect();
        Ok(Orients { q, laue, phase })
    }
    pub fn get(&self, c: usize) -> Option<(Quat, Laue)> {
        Laue::from_code(self.laue[c] as u32).map(|l| (self.q[c].map(|v| v as f64), l))
    }
}

/// Misorientation angle (radians) of two orientations of the same class.
pub fn miso(q1: Quat, q2: Quat, l: Laue) -> f64 {
    cx::disorientation_angle(q1, q2, l)
}

fn with_cell(img: &ImageData, arrays: Vec<Array>) -> ImageData {
    let mut out = img.clone();
    for a in arrays {
        out.cell_data.insert(a);
    }
    out
}

// ------------------------------------------------------------------ segmentation

pub const SEG_MISORIENTATION: &str = "Misorientation";
pub const SEG_CAXIS: &str = "C-axis alignment";
pub const SEG_SCALAR: &str = "Scalar";

/// Segment Features: group face-connected cells into features. Misorientation: neighbours of
/// the same phase whose misorientation is below `tolerance` (degrees). C-axis alignment:
/// neighbours whose crystal c axes are within `tolerance` degrees (either way round). Scalar:
/// neighbours whose values of `scalar` differ by at most `tolerance`. Cells outside `mask`, and
/// (for the orientation methods) cells without a crystal class, get feature 0.
#[allow(clippy::too_many_arguments)]
pub fn segment(
    ds: &DataSet,
    method: &str,
    tolerance: f64,
    mask: Option<&str>,
    orientations: Option<(Association, String)>,
    structure: &str,
    phases: Option<&str>,
    scalar: Option<(Association, String)>,
    shuffle_seed: Option<u64>,
) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let n = v.len();
    let mask = mask_of(img, mask)?;
    let ok = |c: usize| mask.as_ref().is_none_or(|m| m[c]);
    let tol = tolerance;
    let ors = if method == SEG_SCALAR { None } else { Some(Orients::of(ds, orientations, structure, phases)?) };
    let sc: Option<Vec<f64>> = if method == SEG_SCALAR {
        let (_, name) = scalar.ok_or_else(|| anyhow!("choose the scalar array"))?;
        let a = img.cell_data.get(&name).ok_or_else(|| anyhow!("no cell array {name}"))?;
        if a.ncomp != 1 {
            bail!("{name} has {} components; Scalar segmentation needs one", a.ncomp);
        }
        Some((0..a.len()).map(|k| a.get(k, 0)).collect())
    } else {
        None
    };
    // C axes in sample coordinates (the third row of the orientation matrix).
    let caxis: Option<Vec<[f64; 3]>> = (method == SEG_CAXIS).then(|| {
        let o = ors.as_ref().unwrap();
        (0..n)
            .map(|c| {
                let m = cx::quat_to_matrix(o.q[c].map(|v| v as f64));
                m[2]
            })
            .collect()
    });
    let eligible = |c: usize| ok(c) && ors.as_ref().is_none_or(|o| o.laue[c] != 255);
    let tol_rad = tol.to_radians();
    let cos_half = (tol_rad * 0.5).cos();
    let group = |a: usize, b: usize| -> bool {
        match (&ors, &sc) {
            (_, Some(s)) => (s[a] - s[b]).abs() <= tol,
            (Some(o), None) => {
                if o.phase[a] != o.phase[b] || o.laue[a] != o.laue[b] {
                    return false;
                }
                if let Some(ca) = &caxis {
                    let d = (ca[a][0] * ca[b][0] + ca[a][1] * ca[b][1] + ca[a][2] * ca[b][2]).clamp(-1.0, 1.0);
                    let w = d.acos();
                    return w <= tol_rad || std::f64::consts::PI - w <= tol_rad;
                }
                let (Some((qa, l)), Some((qb, _))) = (o.get(a), o.get(b)) else { return false };
                // Strictly below the tolerance: the largest |w| of the symmetric equivalents.
                let d = cx::mul(qb, cx::conj(qa));
                l.operators().iter().any(|s| (s[0] * d[0] - s[1] * d[1] - s[2] * d[2] - s[3] * d[3]).abs() > cos_half)
            }
            _ => false,
        }
    };
    let mut ids = vec![0i32; n];
    let mut gnum = 0i32;
    let mut stack = Vec::new();
    let mut feat_phase = vec![0i32];
    for seed in 0..n {
        if ids[seed] != 0 || !eligible(seed) {
            continue;
        }
        gnum += 1;
        ids[seed] = gnum;
        feat_phase.push(ors.as_ref().map(|o| o.phase[seed]).unwrap_or(1));
        stack.push(seed);
        while let Some(c) = stack.pop() {
            for nb in v.nbrs(c).into_iter().flatten() {
                if ids[nb] == 0 && eligible(nb) && group(c, nb) {
                    ids[nb] = gnum;
                    stack.push(nb);
                }
            }
        }
    }
    if let Some(seed) = shuffle_seed {
        // A fixed shuffle of ids 1..N (neighbouring features get unrelated numbers, which
        // colors better).
        let mut perm: Vec<i32> = (0..=gnum).collect();
        let mut s = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
        for i in (2..perm.len()).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let j = 1 + (s % i as u64) as usize;
            perm.swap(i, j);
        }
        let mut fp = vec![0; feat_phase.len()];
        for (old, new) in perm.iter().enumerate() {
            fp[*new as usize] = feat_phase[old];
        }
        feat_phase = fp;
        for id in ids.iter_mut() {
            *id = perm[*id as usize];
        }
    }
    let mut out = img.clone();
    // Feature arrays of an earlier segmentation no longer apply.
    if let Some(old) = img.cell_data.get("FeatureIds") {
        let m = (0..old.len()).map(|k| old.get(k, 0) as i64).max().unwrap_or(0).max(0) as usize + 1;
        for name in feature_arrays(img, m) {
            out.field_data.remove(&name);
        }
    }
    out.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(ids)));
    let nf = gnum as usize + 1;
    out.field_data.insert(Array::new("Active", 1, ArrayData::U8((0..nf).map(|i| (i > 0) as u8).collect())));
    out.field_data.insert(Array::new("FeaturePhases", 1, ArrayData::I32(feat_phase)));
    Ok(DataSet::ImageData(out))
}

// ------------------------------------------------------------------ clean-up

/// Fill cells marked -1 from their face neighbours: each sweep gives every marked cell the
/// values (all cell arrays) of the neighbour whose feature most of its neighbours belong to
/// (ties: the feature that reached the count first, in the order -z, -y, -x, +x, +y, +z), among
/// neighbours with ids >= `min_id`. Sweeps repeat until no cell is marked or no progress.
fn fill_marked(img: &mut ImageData, ids_name: &str, mut ids: Vec<i32>, min_id: i32) {
    let v = Vox::of(img);
    let n = ids.len();
    loop {
        let marked: Vec<usize> = (0..n).filter(|c| ids[*c] == -1).collect();
        if marked.is_empty() {
            break;
        }
        let choice: Vec<(usize, Option<usize>)> = marked
            .par_iter()
            .map(|&c| {
                let mut tally: Vec<(i32, usize)> = Vec::with_capacity(6);
                let (mut most, mut pick) = (0usize, None);
                for nb in v.nbrs(c).into_iter().flatten() {
                    let f = ids[nb];
                    if f >= min_id && f != -1 {
                        let cnt = match tally.iter_mut().find(|t| t.0 == f) {
                            Some(t) => {
                                t.1 += 1;
                                t.1
                            }
                            None => {
                                tally.push((f, 1));
                                1
                            }
                        };
                        if cnt > most {
                            most = cnt;
                            pick = Some(nb);
                        }
                    }
                }
                (c, pick)
            })
            .collect();
        let mut src: Vec<usize> = (0..n).collect();
        let mut progress = false;
        for (c, p) in &choice {
            if let Some(p) = p {
                src[*c] = *p;
                progress = true;
            }
        }
        if !progress {
            for c in marked {
                ids[c] = 0;
            }
            break;
        }
        // Copy every cell array from the chosen neighbours (the ids among them).
        let names: Vec<String> = img.cell_data.iter().map(|a| a.name.clone()).collect();
        for name in names {
            if name == ids_name {
                continue;
            }
            let a = img.cell_data.get(&name).unwrap();
            let g = a.gather(&src);
            img.cell_data.insert(g);
        }
        ids = src.iter().map(|s| ids[*s]).collect();
    }
    img.cell_data.insert(Array::new(ids_name, 1, ArrayData::I32(ids)));
}

/// Fill Bad Data: clusters of cells without a feature (id 0) smaller than `min_defect` cells
/// are filled from the surrounding features (every cell array copied from a neighbour);
/// larger clusters stay 0.
pub fn fill_bad_data(ds: &DataSet, min_defect: usize, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let mut ids = cell_i32(img, ids_name)?;
    let n = ids.len();
    let mut seen = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    for c in 0..n {
        if ids[c] != 0 || seen[c] {
            continue;
        }
        let mut cluster = vec![c];
        seen[c] = true;
        queue.push_back(c);
        while let Some(x) = queue.pop_front() {
            for nb in v.nbrs(x).into_iter().flatten() {
                if ids[nb] == 0 && !seen[nb] {
                    seen[nb] = true;
                    cluster.push(nb);
                    queue.push_back(nb);
                }
            }
        }
        if cluster.len() < min_defect {
            for x in cluster {
                ids[x] = -1;
            }
        }
    }
    let mut out = img.clone();
    fill_marked(&mut out, ids_name, ids, 1);
    Ok(DataSet::ImageData(out))
}

/// Keep `rows` of the per-feature field arrays (`n_old` rows) in that order.
fn keep_feature_rows(img: &mut ImageData, n_old: usize, rows: &[usize]) {
    for name in feature_arrays(img, n_old) {
        let a = img.field_data.get(&name).unwrap().gather(rows);
        img.field_data.insert(a);
    }
}

/// Minimum Size: features with fewer than `min_cells` cells are removed and their cells
/// filled from the neighbouring features (or left without a feature when only id-0 cells
/// border them); the remaining features are numbered from 1 in their old order, with their
/// rows of the per-feature field arrays.
pub fn minimum_size(ds: &DataSet, min_cells: usize, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let mut ids = cell_i32(img, ids_name)?;
    let nf = rows(&ids);
    let mut count = vec![0usize; nf];
    for &i in &ids {
        if i > 0 {
            count[i as usize] += 1;
        }
    }
    let keep: Vec<bool> = (0..nf).map(|f| f == 0 || count[f] >= min_cells).collect();
    if (1..nf).all(|f| !keep[f]) && nf > 1 {
        bail!("no feature has {min_cells} cells or more");
    }
    for id in ids.iter_mut() {
        if *id > 0 && !keep[*id as usize] {
            *id = -1;
        }
    }
    let mut out = img.clone();
    fill_marked(&mut out, ids_name, ids, 0);
    let ids = cell_i32(&out, ids_name)?;
    let rows_kept: Vec<usize> = (0..nf).filter(|f| keep[*f]).collect();
    let mut new_id = vec![0i32; nf];
    for (k, f) in rows_kept.iter().enumerate() {
        new_id[*f] = k as i32;
    }
    out.cell_data.insert(Array::new(ids_name, 1, ArrayData::I32(ids.iter().map(|i| if *i > 0 { new_id[*i as usize] } else { 0 }).collect())));
    keep_feature_rows(&mut out, nf, &rows_kept);
    Ok(DataSet::ImageData(out))
}

// ------------------------------------------------------------------ sizes, centroids, shapes

/// Feature Sizes: NumElements, Volumes and EquivalentDiameters (the diameter of the sphere of
/// the same volume; for flat maps, of the circle of the same area).
pub fn feature_sizes(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = cell_i32(img, ids_name)?;
    let nf = rows(&ids);
    let mut count = vec![0i32; nf];
    for &i in &ids {
        if i > 0 {
            count[i as usize] += 1;
        }
    }
    let two_d = v.is_2d();
    let cell = if two_d { (0..3).filter(|a| !(v.flat[*a] || v.n[*a] == 1)).map(|a| v.sp[a]).product::<f64>() } else { v.sp.iter().product() };
    let vol: Vec<f64> = count.iter().map(|c| *c as f64 * cell).collect();
    let esd: Vec<f64> = vol.iter().map(|v| if two_d { 2.0 * (v / std::f64::consts::PI).sqrt() } else { 2.0 * (v * 3.0 / (4.0 * std::f64::consts::PI)).cbrt() }).collect();
    let mut out = img.clone();
    out.field_data.insert(Array::new("NumElements", 1, ArrayData::I32(count)));
    out.field_data.insert(Array::scalars_f64("Volumes", vol));
    out.field_data.insert(Array::scalars_f64("EquivalentDiameters", esd));
    Ok(DataSet::ImageData(out))
}

fn centroids_of(v: &Vox, ids: &[i32], nf: usize) -> Vec<[f64; 3]> {
    let mut sum = vec![[0.0f64; 3]; nf];
    let mut cnt = vec![0usize; nf];
    for (c, &i) in ids.iter().enumerate() {
        if i >= 0 {
            let p = v.center(c);
            for k in 0..3 {
                sum[i as usize][k] += p[k];
            }
            cnt[i as usize] += 1;
        }
    }
    sum.iter().zip(&cnt).map(|(s, n)| if *n > 0 { s.map(|x| x / *n as f64) } else { [0.0; 3] }).collect()
}

/// Feature Centroids: the mean of each feature's cell centres (Centroids).
pub fn feature_centroids(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = cell_i32(img, ids_name)?;
    let c = centroids_of(&v, &ids, rows(&ids));
    let mut out = img.clone();
    out.field_data.insert(Array::new("Centroids", 3, ArrayData::F64(c.concat())));
    Ok(DataSet::ImageData(out))
}

/// How the second moments of a feature are summed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Moments {
    /// Exact moments of the cells: centres at (i + 1/2) h, each cell adding h^2/12 per axis.
    Exact,
    /// Cells placed half a cell low (at their corner) and sampled at 8 points a quarter of a
    /// cell off, as one widely used implementation does; kept for comparisons.
    CornerQuarter,
}

/// Eigenvalues (descending) and eigenvectors (columns, matching) of a symmetric 3x3 matrix,
/// by Jacobi rotations.
pub fn sym_eigen(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..60 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-300 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            let mut b = a;
            for k in 0..3 {
                b[k][p] = c * a[k][p] - s * a[k][q];
                b[k][q] = s * a[k][p] + c * a[k][q];
            }
            let mut d = b;
            for k in 0..3 {
                d[p][k] = c * b[p][k] - s * b[q][k];
                d[q][k] = s * b[p][k] + c * b[q][k];
            }
            a = d;
            for row in v.iter_mut() {
                let (x, y) = (row[p], row[q]);
                row[p] = c * x - s * y;
                row[q] = s * x + c * y;
            }
        }
    }
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|i, j| a[*j][*j].total_cmp(&a[*i][*i]));
    let vals = idx.map(|i| a[i][i]);
    let vecs: [[f64; 3]; 3] = std::array::from_fn(|r| std::array::from_fn(|c| v[r][idx[c]]));
    (vals, vecs)
}

/// Shape of one feature from its inertia tensor about the centroid (physical units):
/// semi-axis lengths a >= b >= c of the ellipsoid with the same moments, the rows of the
/// orientation matrix (principal directions of a, b, c) and Omega3.
pub struct Shape3 {
    pub axes: [f64; 3],
    pub dirs: [[f64; 3]; 3],
    pub omega3: f64,
}

pub fn shapes(v: &Vox, ids: &[i32], scheme: Moments, cents: &[[f64; 3]]) -> Vec<Option<Shape3>> {
    let nf = cents.len();
    let mut m = vec![[0.0f64; 6]; nf];
    let mut cnt = vec![0usize; nf];
    let (shift, var) = match scheme {
        Moments::Exact => (0.5, 1.0 / 12.0),
        Moments::CornerQuarter => (0.0, 1.0 / 16.0),
    };
    let s = v.sp;
    for (c, &f) in ids.iter().enumerate() {
        if f <= 0 {
            continue;
        }
        let p = v.ijk(c);
        let x: [f64; 3] = std::array::from_fn(|a| v.corner[a] + (p[a] as f64 + shift) * s[a] - cents[f as usize][a]);
        let w: [f64; 3] = std::array::from_fn(|a| s[a] * s[a] * var);
        let e = &mut m[f as usize];
        e[0] += x[1] * x[1] + w[1] + x[2] * x[2] + w[2];
        e[1] += x[0] * x[0] + w[0] + x[2] * x[2] + w[2];
        e[2] += x[0] * x[0] + w[0] + x[1] * x[1] + w[1];
        e[3] += x[0] * x[1];
        e[4] += x[1] * x[2];
        e[5] += x[0] * x[2];
        cnt[f as usize] += 1;
    }
    let cell: f64 = s.iter().product();
    (0..nf)
        .map(|f| {
            if f == 0 || cnt[f] == 0 {
                return None;
            }
            let k = cell;
            let (m0, m1, m2) = (m[f][0] * k, m[f][1] * k, m[f][2] * k);
            let (m3, m4, m5) = (-m[f][3] * k, -m[f][4] * k, -m[f][5] * k);
            let (r, vecs) = sym_eigen([[m0, m3, m5], [m3, m1, m4], [m5, m4, m2]]);
            let mult = 1.0 / (4.0 * std::f64::consts::PI);
            let i = r.map(|x| 15.0 * x * mult);
            let a_ = (i[0] + i[1] - i[2]) / 2.0;
            let b_ = (i[0] + i[2] - i[1]) / 2.0;
            let c_ = (i[1] + i[2] - i[0]) / 2.0;
            let (a, b, c) = if a_ > 0.0 && b_ > 0.0 && c_ > 0.0 {
                let a = (a_.powi(4) / (b_ * c_)).powf(0.1);
                let b = (b_ / a_).sqrt() * a;
                (a, b, a_ / (a.powi(3) * b))
            } else {
                (0.0, 0.0, 0.0)
            };
            // Omega3 from the central second moments.
            let (u200, u020, u002) = ((m1 + m2 - m0) / 2.0, (m0 + m2 - m1) / 2.0, (m0 + m1 - m2) / 2.0);
            let (u110, u011, u101) = (-m3, -m4, -m5);
            let o3 = u200 * u020 * u002 + 2.0 * u110 * u101 * u011 - u200 * u011 * u011 - u020 * u101 * u101 - u002 * u110 * u110;
            let vol = cnt[f] as f64 * cell;
            let sphere = 2000.0 * std::f64::consts::PI * std::f64::consts::PI / 9.0;
            let omega3 = if o3 > 0.0 { (vol.powi(5) / o3 / sphere).min(1.0) } else { 0.0 };
            // Rows: principal directions of the longest (smallest inertia), middle, shortest axes.
            let col = |j: usize| [vecs[0][j], vecs[1][j], vecs[2][j]];
            let mut dirs = [col(2), col(1), col(0)];
            let det = dirs[0][0] * (dirs[1][1] * dirs[2][2] - dirs[1][2] * dirs[2][1]) - dirs[0][1] * (dirs[1][0] * dirs[2][2] - dirs[1][2] * dirs[2][0]) + dirs[0][2] * (dirs[1][0] * dirs[2][1] - dirs[1][1] * dirs[2][0]);
            if det < 0.0 {
                dirs[2] = dirs[2].map(|x| -x);
            }
            Some(Shape3 { axes: [a, b, c], dirs, omega3 })
        })
        .collect()
}

/// Feature Shapes: AxisLengths (semi-axes a >= b >= c of the equal-moment ellipsoid),
/// AspectRatios (b/a, c/a), AxisEulerAngles (Bunge angles of the axes' directions) and
/// Omega3s (1 for an ellipsoid, smaller for other shapes). For flat maps: the equal-moment
/// ellipse (a, b, 0), (b/a, 0), its in-plane angle as the first Euler angle, Omega3 0.
pub fn feature_shapes(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = cell_i32(img, ids_name)?;
    let nf = rows(&ids);
    let cents = centroids_of(&v, &ids, nf);
    let mut axes = vec![0.0; 3 * nf];
    let mut ar = vec![0.0; 2 * nf];
    let mut eul = vec![0.0; 3 * nf];
    let mut om = vec![0.0; nf];
    if v.is_2d() {
        // In-plane axes: the two non-flat ones.
        let pl: Vec<usize> = (0..3).filter(|a| !(v.flat[*a] || v.n[*a] == 1)).collect();
        let (pa, pb) = (pl.first().copied().unwrap_or(0), pl.get(1).copied().unwrap_or(1));
        let mut m = vec![[0.0f64; 3]; nf];
        let mut cnt = vec![0usize; nf];
        for (c, &f) in ids.iter().enumerate() {
            if f <= 0 {
                continue;
            }
            let p = v.center(c);
            let (x, y) = (p[pa] - cents[f as usize][pa], p[pb] - cents[f as usize][pb]);
            let e = &mut m[f as usize];
            e[0] += y * y + v.sp[pb] * v.sp[pb] / 12.0;
            e[1] += x * x + v.sp[pa] * v.sp[pa] / 12.0;
            e[2] -= x * y;
            cnt[f as usize] += 1;
        }
        let area = v.sp[pa] * v.sp[pb];
        for f in 1..nf {
            if cnt[f] == 0 {
                continue;
            }
            let (ixx, iyy, ixy) = (m[f][0] * area, m[f][1] * area, m[f][2] * area);
            let tr = 0.5 * (ixx + iyy);
            let dd = (0.25 * (ixx - iyy) * (ixx - iyy) + ixy * ixy).sqrt();
            let (r1, r2) = (tr + dd, tr - dd);
            let pre = (4.0 / std::f64::consts::PI).powf(0.25);
            let (a, b) = if r2 > 0.0 { (pre * (r1.powi(3) / r2).powf(0.125), pre * (r2.powi(3) / r1).powf(0.125)) } else { (0.0, 0.0) };
            axes[3 * f] = a;
            axes[3 * f + 1] = b;
            ar[2 * f] = if a > 0.0 { b / a } else { 0.0 };
            // Major axis: eigenvector of the larger inertia r1 is the axis about which the
            // feature is widest, i.e. perpendicular to it lies the long direction.
            let ang = 0.5 * (2.0 * ixy).atan2(ixx - iyy);
            eul[3 * f] = (ang + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::PI);
        }
    } else {
        for (f, s) in shapes(&v, &ids, Moments::Exact, &cents).into_iter().enumerate() {
            let Some(s) = s else { continue };
            axes[3 * f..3 * f + 3].copy_from_slice(&s.axes);
            if s.axes[0] > 0.0 {
                ar[2 * f] = s.axes[1] / s.axes[0];
                ar[2 * f + 1] = s.axes[2] / s.axes[0];
            }
            let e = cx::matrix_to_euler(&s.dirs);
            eul[3 * f..3 * f + 3].copy_from_slice(&e);
            om[f] = s.omega3;
        }
    }
    let mut out = img.clone();
    out.field_data.insert(Array::new("AxisLengths", 3, ArrayData::F64(axes)));
    out.field_data.insert(Array::new("AspectRatios", 2, ArrayData::F64(ar)));
    out.field_data.insert(Array::new("AxisEulerAngles", 3, ArrayData::F64(eul)));
    out.field_data.insert(Array::scalars_f64("Omega3s", om));
    Ok(DataSet::ImageData(out))
}

// ------------------------------------------------------------------ orientations

/// Average orientation of each feature: each cell's symmetry equivalent nearest the running
/// average (the identity for the first cell) is added; the sum is normalised.
pub fn average_quats(o: &Orients, ids: &[i32], nf: usize) -> Vec<Option<Quat>> {
    let mut sum = vec![[0.0f64; 4]; nf];
    let mut cnt = vec![0usize; nf];
    let mut laue: Vec<Option<Laue>> = vec![None; nf];
    for (c, &f) in ids.iter().enumerate() {
        if f <= 0 {
            continue;
        }
        let Some((q, l)) = o.get(c) else { continue };
        let f = f as usize;
        cnt[f] += 1;
        let cur = if cnt[f] == 1 { cx::IDENTITY } else { sum[f].map(|x| x / cnt[f] as f64) };
        let mut best = (f64::NEG_INFINITY, q);
        for s in l.operators() {
            let p = cx::positive(cx::mul(*s, q));
            let d = p[0] * cur[0] + p[1] * cur[1] + p[2] * cur[2] + p[3] * cur[3];
            if d > best.0 {
                best = (d, p);
            }
        }
        for k in 0..4 {
            sum[f][k] += best.1[k];
        }
        laue[f] = Some(l);
    }
    (0..nf).map(|f| (f > 0 && cnt[f] > 0).then(|| cx::positive(cx::normalize(sum[f])))).collect()
}

/// Average Orientations: AvgQuats (x, y, z, w) and AvgEulerAngles per feature.
pub fn average_orientations(ds: &DataSet, ids_name: &str, orientations: Option<(Association, String)>, structure: &str, phases: Option<&str>) -> Result<DataSet> {
    let img = image(ds)?;
    let ids = cell_i32(img, ids_name)?;
    let o = Orients::of(ds, orientations, structure, phases)?;
    let nf = rows(&ids);
    let avg = average_quats(&o, &ids, nf);
    let quats: Vec<f64> = avg.iter().flat_map(|q| q.map(cx::quat_to_xyzw).unwrap_or([0.0, 0.0, 0.0, 1.0])).collect();
    let eul: Vec<f64> = avg.iter().flat_map(|q| q.map(cx::quat_to_euler).unwrap_or([0.0; 3])).collect();
    let mut out = img.clone();
    out.field_data.insert(Array::new("AvgQuats", 4, ArrayData::F64(quats)));
    out.field_data.insert(Array::new("AvgEulerAngles", 3, ArrayData::F64(eul)));
    Ok(DataSet::ImageData(out))
}

fn avg_quats_field(img: &ImageData, nf: usize) -> Result<Vec<Option<Quat>>> {
    let a = img.field_data.get("AvgQuats").ok_or_else(|| anyhow!("no AvgQuats field array (run Average Orientations first)"))?;
    if a.len() < nf || a.ncomp != 4 {
        bail!("AvgQuats does not have a row per feature");
    }
    Ok((0..nf).map(|f| (f > 0).then(|| cx::quat_from_xyzw([a.get(f, 0), a.get(f, 1), a.get(f, 2), a.get(f, 3)]))).collect())
}

/// Reference Misorientations: each cell's misorientation (degrees) from its feature's average
/// orientation (FeatureReferenceMisorientations) and the feature means
/// (FeatureAvgMisorientations).
pub fn reference_misorientations(ds: &DataSet, ids_name: &str, orientations: Option<(Association, String)>, structure: &str, phases: Option<&str>) -> Result<DataSet> {
    let img = image(ds)?;
    let ids = cell_i32(img, ids_name)?;
    let o = Orients::of(ds, orientations, structure, phases)?;
    let nf = rows(&ids);
    let avg = match avg_quats_field(img, nf) {
        Ok(a) => a,
        Err(_) => average_quats(&o, &ids, nf),
    };
    let cell: Vec<f64> = (0..ids.len())
        .into_par_iter()
        .map(|c| {
            let f = ids[c];
            match (f > 0, o.get(c)) {
                (true, Some((q, l))) => avg[f as usize].map(|a| miso(q, a, l).to_degrees()).unwrap_or(0.0),
                _ => 0.0,
            }
        })
        .collect();
    let mut sum = vec![0.0; nf];
    let mut cnt = vec![0usize; nf];
    for c in 0..ids.len() {
        if ids[c] > 0 && o.get(c).is_some() {
            sum[ids[c] as usize] += cell[c];
            cnt[ids[c] as usize] += 1;
        }
    }
    let mean: Vec<f64> = sum.iter().zip(&cnt).map(|(s, n)| if *n > 0 { s / *n as f64 } else { 0.0 }).collect();
    let mut out = with_cell(img, vec![Array::scalars_f64("FeatureReferenceMisorientations", cell)]);
    out.field_data.insert(Array::scalars_f64("FeatureAvgMisorientations", mean));
    Ok(DataSet::ImageData(out))
}

/// Kernel Average Misorientation of each cell (degrees): the mean misorientation to the other
/// cells of the same feature in the kernel (offsets `lo[a]..=hi[a]` cells along each axis), the
/// cell itself counted too when `with_self`. 0 for cells without a feature, orientation or
/// neighbour.
pub fn kam_kernel(v: &Vox, o: &Orients, ids: &[i32], lo: [i64; 3], hi: [i64; 3], with_self: bool) -> Vec<f64> {
    (0..ids.len())
        .into_par_iter()
        .map(|c| {
            let f = ids[c];
            let Some((q, l)) = o.get(c) else { return 0.0 };
            if f <= 0 {
                return 0.0;
            }
            let p = v.ijk(c);
            let (mut sum, mut n) = (0.0, 0usize);
            for dz in lo[2]..=hi[2] {
                for dy in lo[1]..=hi[1] {
                    for dx in lo[0]..=hi[0] {
                        let r = [p[0] as i64 + dx, p[1] as i64 + dy, p[2] as i64 + dz];
                        if (0..3).any(|a| r[a] < 0 || r[a] >= v.n[a] as i64) {
                            continue;
                        }
                        let nb = (r[2] as usize * v.n[1] + r[1] as usize) * v.n[0] + r[0] as usize;
                        if ids[nb] != f || (nb == c && !with_self) {
                            continue;
                        }
                        n += 1;
                        if nb != c {
                            if let Some((q2, _)) = o.get(nb) {
                                sum += miso(q, q2, l).to_degrees();
                            }
                        }
                    }
                }
            }
            if n > 0 {
                sum / n as f64
            } else {
                0.0
            }
        })
        .collect()
}

pub fn kernel_average_misorientation(ds: &DataSet, ids_name: &str, kernel: [i64; 3], orientations: Option<(Association, String)>, structure: &str, phases: Option<&str>) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = cell_i32(img, ids_name)?;
    let o = Orients::of(ds, orientations, structure, phases)?;
    let k = kernel.map(|x| x.max(0));
    let kam = kam_kernel(&v, &o, &ids, k.map(|x| -x), k, true);
    Ok(DataSet::ImageData(with_cell(img, vec![Array::scalars_f64("KernelAverageMisorientations", kam)])))
}

// ------------------------------------------------------------------ neighbours and boundaries

/// Pairs of neighbouring features (a < b) with the number of faces they share.
fn neighbour_pairs(v: &Vox, ids: &[i32]) -> std::collections::BTreeMap<(i32, i32), usize> {
    let mut m = std::collections::BTreeMap::new();
    for c in 0..ids.len() {
        let f = ids[c];
        if f <= 0 {
            continue;
        }
        // Each face once: the +x, +y, +z neighbours.
        for nb in v.nbrs(c)[3..].iter().flatten() {
            let g = ids[*nb];
            if g > 0 && g != f {
                *m.entry((f.min(g), f.max(g))).or_insert(0) += 1;
            }
        }
    }
    m
}

/// Feature Neighbors: NumNeighbors and SurfaceFeatures (1 for features touching the edge of
/// the volume or a cell without a feature) per feature, BoundaryCells per cell (how many of its
/// face neighbours belong to another feature), and, when average orientations exist,
/// AvgMisorientations (the mean misorientation, degrees, to the neighbouring features of the
/// same crystal class).
pub fn feature_neighbors(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = cell_i32(img, ids_name)?;
    let nf = rows(&ids);
    let pairs = neighbour_pairs(&v, &ids);
    let mut num = vec![0i32; nf];
    for (a, b) in pairs.keys() {
        num[*a as usize] += 1;
        num[*b as usize] += 1;
    }
    let mut surf = vec![0u8; nf];
    let boundary: Vec<i8> = (0..ids.len())
        .map(|c| {
            let f = ids[c];
            if f <= 0 {
                return 0;
            }
            let mut k = 0;
            for nb in v.nbrs(c).into_iter().flatten() {
                let g = ids[nb];
                if g != f && g > 0 {
                    k += 1;
                }
            }
            k
        })
        .collect();
    for c in 0..ids.len() {
        let f = ids[c];
        if f > 0 && (v.on_boundary(c) || v.nbrs(c).into_iter().flatten().any(|nb| ids[nb] == 0)) {
            surf[f as usize] = 1;
        }
    }
    let mut out = with_cell(img, vec![Array::new("BoundaryCells", 1, ArrayData::I8(boundary))]);
    out.field_data.insert(Array::new("NumNeighbors", 1, ArrayData::I32(num)));
    out.field_data.insert(Array::new("SurfaceFeatures", 1, ArrayData::U8(surf)));
    if let Ok(avg) = avg_quats_field(img, nf) {
        let laue = feature_laue(img, nf);
        let mut sum = vec![0.0; nf];
        let mut cnt = vec![0usize; nf];
        for (a, b) in pairs.keys() {
            let (a, b) = (*a as usize, *b as usize);
            if let (Some(qa), Some(qb), Some(la), Some(lb)) = (avg[a], avg[b], laue[a], laue[b]) {
                if la == lb {
                    let w = miso(qa, qb, la).to_degrees();
                    sum[a] += w;
                    sum[b] += w;
                    cnt[a] += 1;
                    cnt[b] += 1;
                }
            }
        }
        out.field_data.insert(Array::scalars_f64("AvgMisorientations", sum.iter().zip(&cnt).map(|(s, n)| if *n > 0 { s / *n as f64 } else { f64::NAN }).collect()));
    }
    Ok(DataSet::ImageData(out))
}

/// The crystal class of each feature (from FeaturePhases and CrystalStructures, or phase 1).
fn feature_laue(img: &ImageData, nf: usize) -> Vec<Option<Laue>> {
    let cs: Vec<u32> = img.field_data.get("CrystalStructures").map(|a| (0..a.len()).map(|i| a.get(i, 0) as u32).collect()).unwrap_or_default();
    let fp = img.field_data.get("FeaturePhases").or_else(|| img.field_data.get("Phases").filter(|a| a.len() == nf));
    (0..nf)
        .map(|f| {
            if f == 0 {
                return None;
            }
            let p = fp.map(|a| a.get(f, 0) as usize).unwrap_or(1);
            cs.get(p).and_then(|c| Laue::from_code(*c))
        })
        .collect()
}

/// Exact Euclidean distance transform (squared, in cells weighted by spacing) from `seed` cells
/// over the grid (separable, one axis at a time).
pub fn distance_transform(v: &Vox, seed: &[bool]) -> Vec<f64> {
    let inf = f64::INFINITY;
    let mut d: Vec<f64> = seed.iter().map(|s| if *s { 0.0 } else { inf }).collect();
    for axis in 0..3 {
        let n = v.n[axis];
        if n == 1 {
            continue;
        }
        let h2 = v.sp[axis] * v.sp[axis];
        let stride = [1, v.n[0], v.n[0] * v.n[1]][axis];
        let lines: Vec<usize> = (0..v.len()).filter(|c| v.ijk(*c)[axis] == 0).collect();
        let results: Vec<(usize, Vec<f64>)> = lines
            .par_iter()
            .map(|&start| {
                let f: Vec<f64> = (0..n).map(|i| d[start + i * stride]).collect();
                (start, lower_envelope(&f, h2))
            })
            .collect();
        for (start, line) in results {
            for (i, x) in line.into_iter().enumerate() {
                d[start + i * stride] = x;
            }
        }
    }
    d
}

/// 1-D squared distance transform with sample spacing^2 = h2 (Felzenszwalb and Huttenlocher).
fn lower_envelope(f: &[f64], h2: f64) -> Vec<f64> {
    let n = f.len();
    let mut out = vec![f64::INFINITY; n];
    let pts: Vec<usize> = (0..n).filter(|i| f[*i].is_finite()).collect();
    if pts.is_empty() {
        return out;
    }
    let mut v: Vec<usize> = Vec::new();
    let mut z: Vec<f64> = Vec::new();
    let s = |q: usize, p: usize| ((f[q] + h2 * (q * q) as f64) - (f[p] + h2 * (p * p) as f64)) / (2.0 * h2 * (q as f64 - p as f64));
    for &q in &pts {
        while let Some(&p) = v.last() {
            let x = s(q, p);
            if x <= *z.last().unwrap() {
                v.pop();
                z.pop();
            } else {
                break;
            }
        }
        if v.is_empty() {
            v.push(q);
            z.push(f64::NEG_INFINITY);
        } else {
            let x = s(q, *v.last().unwrap());
            v.push(q);
            z.push(x);
        }
    }
    let mut k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while k + 1 < v.len() && z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        *o = h2 * dq * dq + f[p];
    }
    out
}

/// Distance to Boundaries: for every cell of a feature, the Euclidean distance (model units,
/// between cell centres) to the nearest boundary cell (GBEuclideanDistances), triple-line cell
/// (TJEuclideanDistances) and quadruple-point cell (QPEuclideanDistances). A boundary cell
/// has a face neighbour in another feature (or without one), a triple-line cell neighbours two
/// other features, a quadruple-point cell three. -1 where there is none.
pub fn distance_to_boundaries(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = cell_i32(img, ids_name)?;
    let others: Vec<usize> = (0..ids.len())
        .map(|c| {
            let f = ids[c];
            if f <= 0 {
                return 0;
            }
            let mut seen: Vec<i32> = Vec::new();
            for nb in v.nbrs(c).into_iter().flatten() {
                let g = ids[nb];
                if g != f && g >= 0 && !seen.contains(&g) {
                    seen.push(g);
                }
            }
            seen.len()
        })
        .collect();
    let mut arrays = Vec::new();
    for (k, name) in [(1usize, "GBEuclideanDistances"), (2, "TJEuclideanDistances"), (3, "QPEuclideanDistances")] {
        let seed: Vec<bool> = others.iter().map(|o| *o >= k).collect();
        let d = distance_transform(&v, &seed);
        arrays.push(Array::scalars_f64(name, d.iter().zip(&ids).map(|(d, f)| if *f > 0 && d.is_finite() { d.sqrt() } else { -1.0 }).collect()));
    }
    Ok(DataSet::ImageData(with_cell(img, arrays)))
}

/// Is the misorientation between `a` and `b` (cubic) a twin (60 degrees about <111>) within
/// the tolerances (degrees)?
pub fn is_twin(a: Quat, b: Quat, axis_tol: f64, angle_tol: f64) -> bool {
    let (w, axis) = cx::disorientation(a, b, Laue::CubicHigh);
    let w = w.to_degrees();
    let d111 = ((axis[0].abs() + axis[1].abs() + axis[2].abs()) / 3f64.sqrt()).clamp(-1.0, 1.0).acos().to_degrees();
    d111 < axis_tol && (w - 60.0).abs() < angle_tol
}

/// Merge Twins: neighbouring cubic (m-3m) features whose average orientations are twin related
/// (60 degrees about <111>, within the tolerances) get one parent id: ParentIds per cell and
/// per feature, parents numbered from 1 in the order of their first feature.
pub fn merge_twins(ds: &DataSet, ids_name: &str, axis_tol: f64, angle_tol: f64) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = cell_i32(img, ids_name)?;
    let nf = rows(&ids);
    let avg = avg_quats_field(img, nf)?;
    let laue = feature_laue(img, nf);
    let mut parent: Vec<usize> = (0..nf).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for (a, b) in neighbour_pairs(&v, &ids).keys() {
        let (a, b) = (*a as usize, *b as usize);
        if laue[a] != Some(Laue::CubicHigh) || laue[b] != Some(Laue::CubicHigh) {
            continue;
        }
        if let (Some(qa), Some(qb)) = (avg[a], avg[b]) {
            if is_twin(qa, qb, axis_tol, angle_tol) {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                if ra != rb {
                    parent[ra.max(rb)] = ra.min(rb);
                }
            }
        }
    }
    let mut number = vec![0i32; nf];
    let mut next = 0;
    let mut fparent = vec![0i32; nf];
    for f in 1..nf {
        let r = root(&mut parent, f);
        if number[r] == 0 {
            next += 1;
            number[r] = next;
        }
        fparent[f] = number[r];
    }
    let cells: Vec<i32> = ids.iter().map(|f| if *f > 0 { fparent[*f as usize] } else { 0 }).collect();
    let mut out = with_cell(img, vec![Array::new("ParentIds", 1, ArrayData::I32(cells))]);
    out.field_data.insert(Array::new("ParentIds", 1, ArrayData::I32(fparent)));
    Ok(DataSet::ImageData(out))
}

/// Feature Boundaries: the cell faces between different features as a surface, with
/// FeatureIdA and FeatureIdB (A < B), the misorientation (degrees) of the two cells across each
/// face, and Twin (1 where it is a twin boundary, cubic m-3m, within the tolerances). With
/// `outer`, faces against cells without a feature and the edge of the volume are included
/// (FeatureIdA 0).
pub fn feature_boundaries(ds: &DataSet, ids_name: &str, outer: bool, axis_tol: f64, angle_tol: f64, orientations: Option<(Association, String)>, structure: &str, phases: Option<&str>) -> Result<DataSet> {
    let img = image(ds)?;
    let v = Vox::of(img);
    let ids = cell_i32(img, ids_name)?;
    let o = Orients::of(ds, orientations, structure, phases).ok();
    let mut pts: Vec<f64> = Vec::new();
    let mut map = std::collections::HashMap::new();
    let mut polys = Cells::new();
    let (mut fa, mut fb, mut mis, mut twin) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut point = |p: [usize; 3], pts: &mut Vec<f64>| -> u32 {
        *map.entry(p).or_insert_with(|| {
            pts.extend((0..3).map(|a| v.corner[a] + p[a] as f64 * v.sp[a]));
            (pts.len() / 3 - 1) as u32
        })
    };
    for c in 0..ids.len() {
        let f = ids[c];
        let nb = v.nbrs(c);
        let p = v.ijk(c);
        // Faces toward -x/+x etc.: (neighbour slot, axis, side).
        for (slot, axis, plus) in [(2usize, 0usize, false), (3, 0, true), (1, 1, false), (4, 1, true), (0, 2, false), (5, 2, true)] {
            if v.flat[axis] {
                continue;
            }
            let g = match nb[slot] {
                Some(n) => ids[n],
                None => 0,
            };
            // Interior faces once (from the cell with the smaller index); outer faces from the
            // feature's side.
            let interior = nb[slot].is_some() && g > 0 && f > 0 && g != f;
            let external = outer && f > 0 && g <= 0;
            if !(interior && (nb[slot].unwrap() > c) || external) {
                continue;
            }
            let base = { let mut q = p; if plus { q[axis] += 1; } q };
            let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
            let corners = [base, { let mut q = base; q[u] += 1; q }, { let mut q = base; q[u] += 1; q[w] += 1; q }, { let mut q = base; q[w] += 1; q }];
            let ids4: Vec<u32> = corners.iter().map(|q| point(*q, &mut pts)).collect();
            polys.push(CellType::Quad, &ids4);
            let (a, b) = if external { (0, f) } else { (f.min(g), f.max(g)) };
            fa.push(a);
            fb.push(b);
            let (m, t) = match (&o, nb[slot]) {
                (Some(o), Some(n)) if interior => match (o.get(c), o.get(n)) {
                    (Some((q1, l1)), Some((q2, l2))) if l1 == l2 => {
                        let w = miso(q1, q2, l1).to_degrees();
                        (w, (l1 == Laue::CubicHigh && is_twin(q1, q2, axis_tol, angle_tol)) as u8)
                    }
                    _ => (f64::NAN, 0),
                },
                _ => (f64::NAN, 0),
            };
            mis.push(m);
            twin.push(t);
        }
    }
    let mut pd = PolyData { points: Points::new_f64(pts), polys, ..Default::default() };
    pd.cell_data.insert(Array::new("FeatureIdA", 1, ArrayData::I32(fa)));
    pd.cell_data.insert(Array::new("FeatureIdB", 1, ArrayData::I32(fb)));
    pd.cell_data.insert(Array::scalars_f64("Misorientation", mis));
    pd.cell_data.insert(Array::new("Twin", 1, ArrayData::U8(twin)));
    Ok(DataSet::PolyData(pd))
}

// ------------------------------------------------------------------ tables and colors

/// Feature Table: the per-feature field arrays as a table, one row per feature (id 0 left
/// out), with a FeatureId column.
pub fn feature_table(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let ids = cell_i32(img, ids_name)?;
    let nf = rows(&ids);
    let rows_: Vec<usize> = (1..nf).collect();
    let mut t = Table::default();
    t.columns.insert(Array::new("FeatureId", 1, ArrayData::I32((1..nf as i32).collect())));
    for name in feature_arrays(img, nf) {
        let a = img.field_data.get(&name).unwrap();
        t.columns.insert(a.gather(&rows_));
    }
    Ok(DataSet::Table(t))
}

/// Feature Colors: a distinct, stable 8-bit color per feature id (FeatureColors), black for
/// cells without a feature.
pub fn feature_colors(ds: &DataSet, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let ids = cell_i32(img, ids_name)?;
    let rgb: Vec<u8> = ids
        .iter()
        .flat_map(|f| {
            if *f <= 0 {
                return [0, 0, 0];
            }
            // Golden-ratio hues, with lightness and saturation varying too.
            let h = (*f as f64 * 0.618_033_988_749_895).fract();
            let s = 0.55 + 0.35 * ((*f as f64 * 0.414_213_562).fract());
            let l = 0.45 + 0.25 * ((*f as f64 * 0.732_050_808).fract());
            hsl(h, s, l)
        })
        .collect();
    Ok(DataSet::ImageData(with_cell(img, vec![Array::new("FeatureColors", 3, ArrayData::U8(rgb))])))
}

fn hsl(h: f64, s: f64, l: f64) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h * 6.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eigen_and_distance_transform() {
        let (vals, vecs) = sym_eigen([[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 5.0]]);
        assert!((vals[0] - 5.0).abs() < 1e-12 && (vals[1] - 3.0).abs() < 1e-12 && (vals[2] - 1.0).abs() < 1e-12);
        assert!((vecs[2][0].abs() - 1.0).abs() < 1e-12);
        assert!((vecs[0][1].abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        // Distance transform against brute force on an anisotropic grid.
        let v = Vox { n: [9, 7, 5], sp: [1.0, 1.5, 2.0], corner: [0.0; 3], flat: [false; 3] };
        let mut seed = vec![false; v.len()];
        for c in [3usize, 40, 200, 251] {
            seed[c] = true;
        }
        let d = distance_transform(&v, &seed);
        for c in 0..v.len() {
            let p = v.center(c);
            let want = (0..v.len()).filter(|s| seed[*s]).map(|s| {
                let q = v.center(s);
                (0..3).map(|a| (p[a] - q[a]).powi(2)).sum::<f64>()
            }).fold(f64::INFINITY, f64::min);
            assert!((d[c] - want).abs() < 1e-9, "{c}: {} vs {want}", d[c]);
        }
    }
}
