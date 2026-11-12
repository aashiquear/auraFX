//! Grain structures that look grown rather than tessellated, without a growth simulation.
//!
//! A Voronoi tessellation has flat faces and junctions at whatever angles its seeds give;
//! grown grains have curved faces that meet three at a time at about 120 degrees, sizes
//! spread more widely, and boundaries that wander. Three cheap operations on the labels move
//! a tessellation that way:
//!
//! - **Relaxation** by threshold dynamics: every cell takes the label with the largest
//!   Gaussian-weighted share of the cells around it. One sweep moves a boundary
//!   towards its centre of curvature by about width² / 2 times its curvature (in cells), so a
//!   few sweeps round corners, bring junctions to equal angles and shrink the smallest grains.
//!   On a grid a boundary stops once a sweep would move it by less than half a cell, so the
//!   width sets the scale of what is rounded and sweeps past five or so change little.
//! - **Coarsening**: the smallest grains are removed and their cells given to the neighbours.
//! - **Roughness**: every cell takes its data from a place displaced by a smooth random
//!   field, which makes the boundaries wavy.
//!
//! Each returns, for every cell, the cell of the input it takes its data from, so the
//! orientations and every other cell array follow the labels.

use crate::features::{cell_i32, image, keep_feature_rows, rows, Vox};
use crate::synthetic::{Draws, Rng};
use anyhow::Result;
use aurafx_data::*;
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub struct RealismParams {
    /// Relaxation sweeps.
    pub relax: usize,
    /// Standard deviation of the relaxation's Gaussian, in cells.
    pub width: f64,
    /// Root-mean-square displacement of the roughness, in length units (0: none).
    pub roughness: f64,
    /// Correlation length of the roughness.
    pub roughness_length: f64,
    /// Fraction of the grains removed, smallest first.
    pub coarsen: f64,
    pub periodic: [bool; 3],
    pub seed: u64,
}

impl RealismParams {
    pub fn is_off(&self) -> bool {
        self.relax == 0 && self.roughness <= 0.0 && self.coarsen <= 0.0
    }
}

/// Gaussian weights along each axis out to two widths (they sum to 1 on each axis).
struct Kernel {
    r: [usize; 3],
    w: [Vec<f32>; 3],
}

impl Kernel {
    fn new(width: f64, n: [usize; 3], h: [f64; 3]) -> Kernel {
        let sigma = width.clamp(0.5, 8.0);
        let hmin = (0..3).filter(|a| n[*a] > 1).map(|a| h[a]).fold(f64::INFINITY, f64::min);
        let hmin = if hmin.is_finite() && hmin > 0.0 { hmin } else { 1.0 };
        let rel: [f64; 3] = std::array::from_fn(|a| if h[a] > 0.0 { h[a] / hmin } else { 1.0 });
        let r: [usize; 3] = std::array::from_fn(|a| if n[a] > 1 { ((2.0 * sigma / rel[a]).ceil() as usize).max(1) } else { 0 });
        let w = std::array::from_fn(|a| {
            let raw: Vec<f64> = (-(r[a] as i64)..=r[a] as i64).map(|d| (-(d as f64 * rel[a]).powi(2) / (2.0 * sigma * sigma)).exp()).collect();
            let sum: f64 = raw.iter().sum();
            raw.iter().map(|x| (x / sum) as f32).collect()
        });
        Kernel { r, w }
    }
}

fn step(p: usize, d: i64, n: usize, periodic: bool) -> Option<usize> {
    let v = p as i64 + d;
    if v >= 0 && v < n as i64 {
        Some(v as usize)
    } else if periodic {
        Some(v.rem_euclid(n as i64) as usize)
    } else {
        None
    }
}

/// The cells a grain's smoothed indicator can reach: `len` cells from `lo` along each axis,
/// wrapping round a periodic axis (`round`: the whole axis, joined end to end).
#[derive(Clone, Copy)]
struct Reach {
    lo: [usize; 3],
    len: [usize; 3],
    round: [bool; 3],
}

impl Reach {
    fn cells(&self) -> usize {
        self.len[0] * self.len[1] * self.len[2]
    }
}

/// The reach of every label (None for labels without cells). On a periodic axis a grain cut
/// by the faces is measured in a frame shifted by half the box, where it is whole.
fn reaches(cur: &[i32], n: [usize; 3], periodic: [bool; 3], r: [usize; 3]) -> Vec<Option<Reach>> {
    let nf = rows(cur);
    // Per label and axis: smallest and largest coordinate, as they are and shifted by n / 2.
    let mut ext = vec![[[usize::MAX, 0, usize::MAX, 0]; 3]; nf];
    let half = n.map(|x| x / 2);
    for (c, &l) in cur.iter().enumerate() {
        if l <= 0 {
            continue;
        }
        let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
        let e = &mut ext[l as usize];
        for a in 0..3 {
            let q = (p[a] + half[a]) % n[a];
            e[a] = [e[a][0].min(p[a]), e[a][1].max(p[a]), e[a][2].min(q), e[a][3].max(q)];
        }
    }
    ext.iter()
        .map(|e| {
            if e[0][0] == usize::MAX {
                return None;
            }
            let mut out = Reach { lo: [0; 3], len: [1; 3], round: [false; 3] };
            for a in 0..3 {
                if periodic[a] && n[a] > 1 {
                    let (lo, span) = if e[a][3] - e[a][2] < e[a][1] - e[a][0] {
                        ((e[a][2] + n[a] - half[a]) % n[a], e[a][3] - e[a][2] + 1)
                    } else {
                        (e[a][0], e[a][1] - e[a][0] + 1)
                    };
                    if span + 2 * r[a] >= n[a] {
                        (out.lo[a], out.len[a], out.round[a]) = (0, n[a], true);
                    } else {
                        (out.lo[a], out.len[a]) = ((lo + n[a] - r[a]) % n[a], span + 2 * r[a]);
                    }
                } else {
                    let lo = e[a][0].saturating_sub(r[a]);
                    (out.lo[a], out.len[a]) = (lo, (e[a][1] + r[a]).min(n[a] - 1) - lo + 1);
                }
            }
            Some(out)
        })
        .collect()
}

/// `a` (cells of `len`) smoothed along one axis with weights `w`; cells outside count as 0
/// unless the axis is joined end to end.
fn smooth_axis(a: &[f32], len: [usize; 3], axis: usize, w: &[f32], round: bool) -> Vec<f32> {
    let r = (w.len() / 2) as isize;
    let stride = [1, len[0], len[0] * len[1]][axis];
    let l = len[axis] as isize;
    (0..a.len())
        .map(|c| {
            let i = ((c / stride) % len[axis]) as isize;
            let base = c - i as usize * stride;
            let mut sum = 0.0;
            for d in -r..=r {
                let mut j = i + d;
                if j < 0 || j >= l {
                    if !round {
                        continue;
                    }
                    j = j.rem_euclid(l);
                }
                sum += w[(d + r) as usize] * a[base + j as usize * stride];
            }
            sum
        })
        .collect()
}

/// The smoothed indicator of one label over its reach.
fn smoothed(cur: &[i32], n: [usize; 3], label: i32, b: &Reach, k: &Kernel) -> Vec<f32> {
    let mut a = Vec::with_capacity(b.cells());
    for iz in 0..b.len[2] {
        let z = (b.lo[2] + iz) % n[2];
        for iy in 0..b.len[1] {
            let row = (z * n[1] + (b.lo[1] + iy) % n[1]) * n[0];
            for ix in 0..b.len[0] {
                a.push((cur[row + (b.lo[0] + ix) % n[0]] == label) as u8 as f32);
            }
        }
    }
    for axis in 0..3 {
        if k.r[axis] > 0 {
            a = smooth_axis(&a, b.len, axis, &k.w[axis], b.round[axis]);
        }
    }
    a
}

/// The nearest cell with `label` around `c`, looking further out a cell at a time.
fn donor(cur: &[i32], n: [usize; 3], periodic: [bool; 3], r: [usize; 3], c: usize, label: i32) -> usize {
    let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
    let far = r.iter().copied().max().unwrap_or(1).max(1) as i64;
    for s in 1..=far {
        let lim: [i64; 3] = std::array::from_fn(|a| s.min(r[a] as i64));
        let mut best = (i64::MAX, c);
        for dz in -lim[2]..=lim[2] {
            let Some(z) = step(p[2], dz, n[2], periodic[2]) else { continue };
            for dy in -lim[1]..=lim[1] {
                let Some(y) = step(p[1], dy, n[1], periodic[1]) else { continue };
                for dx in -lim[0]..=lim[0] {
                    if dx.abs().max(dy.abs()).max(dz.abs()) != s {
                        continue;
                    }
                    let Some(x) = step(p[0], dx, n[0], periodic[0]) else { continue };
                    let at = (z * n[1] + y) * n[0] + x;
                    let d2 = dx * dx + dy * dy + dz * dz;
                    if cur[at] == label && d2 < best.0 {
                        best = (d2, at);
                    }
                }
            }
        }
        if best.0 != i64::MAX {
            return best.1;
        }
    }
    c
}

/// One relaxation sweep (threshold dynamics): every label's indicator is smoothed with the
/// Gaussian, and each labelled cell takes the label whose smoothed indicator is largest
/// there (its own on a tie, else the smaller label). Returns the labels and sources after it.
fn sweep(cur: &[i32], src: &[u32], n: [usize; 3], periodic: [bool; 3], k: &Kernel) -> (Vec<i32>, Vec<u32>) {
    /// Cells of smoothed indicators held at a time.
    const BATCH: usize = 48_000_000;
    let reach = reaches(cur, n, periodic, k.r);
    let labels: Vec<usize> = (1..reach.len()).filter(|l| reach[*l].is_some()).collect();
    let total = cur.len();
    let (mut best, mut own, mut lab) = (vec![0f32; total], vec![0f32; total], vec![0i32; total]);
    let mut i = 0;
    while i < labels.len() {
        let (mut j, mut held) = (i, 0usize);
        while j < labels.len() && (j == i || held + reach[labels[j]].unwrap().cells() <= BATCH) {
            held += reach[labels[j]].unwrap().cells();
            j += 1;
        }
        let fields: Vec<Vec<f32>> = labels[i..j].par_iter().map(|l| smoothed(cur, n, *l as i32, &reach[*l].unwrap(), k)).collect();
        for (l, f) in labels[i..j].iter().zip(&fields) {
            let b = reach[*l].unwrap();
            let mut at = 0;
            for iz in 0..b.len[2] {
                let z = (b.lo[2] + iz) % n[2];
                for iy in 0..b.len[1] {
                    let row = (z * n[1] + (b.lo[1] + iy) % n[1]) * n[0];
                    for ix in 0..b.len[0] {
                        let c = row + (b.lo[0] + ix) % n[0];
                        let v = f[at];
                        at += 1;
                        if cur[c] == *l as i32 {
                            own[c] = v;
                        }
                        if v > best[c] {
                            (best[c], lab[c]) = (v, *l as i32);
                        }
                    }
                }
            }
        }
        i = j;
    }
    (0..total)
        .into_par_iter()
        .map(|c| {
            if cur[c] <= 0 || lab[c] <= 0 || lab[c] == cur[c] || own[c] >= best[c] {
                (cur[c], src[c])
            } else {
                (lab[c], src[donor(cur, n, periodic, k.r, c, lab[c])])
            }
        })
        .unzip()
}

/// The smallest `fraction` of the grains given to their neighbours, a layer of cells at a
/// time: a cell takes the label most of its face neighbours outside the removed grains have.
fn coarsen(cur: &mut [i32], src: &mut [u32], n: [usize; 3], periodic: [bool; 3], fraction: f64) {
    let nf = rows(cur);
    let mut count = vec![0usize; nf];
    for &l in cur.iter() {
        if l > 0 {
            count[l as usize] += 1;
        }
    }
    let mut present: Vec<usize> = (1..nf).filter(|f| count[*f] > 0).collect();
    present.sort_by_key(|f| (count[*f], *f));
    let take = ((fraction.clamp(0.0, 1.0) * present.len() as f64).floor() as usize).min(present.len().saturating_sub(1));
    if take == 0 {
        return;
    }
    let mut removed = vec![false; nf];
    for f in &present[..take] {
        removed[*f] = true;
    }
    let stride = [1, n[0], n[0] * n[1]];
    let mut marked: Vec<usize> = (0..cur.len()).filter(|c| cur[*c] > 0 && removed[cur[*c] as usize]).collect();
    while !marked.is_empty() {
        let picks: Vec<Option<usize>> = {
            let cur = &*cur;
            let removed = &removed;
            marked
                .par_iter()
                .map(|&c| {
                    let p = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
                    let mut seen: [(i32, usize, usize); 6] = [(0, 0, 0); 6];
                    let mut m = 0;
                    for a in 0..3 {
                        for d in [-1i64, 1] {
                            let Some(q) = step(p[a], d, n[a], periodic[a]) else { continue };
                            let nb = c - p[a] * stride[a] + q * stride[a];
                            let l = cur[nb];
                            if nb == c || l <= 0 || removed[l as usize] {
                                continue;
                            }
                            match seen[..m].iter_mut().find(|s| s.0 == l) {
                                Some(s) => s.1 += 1,
                                None => {
                                    seen[m] = (l, 1, nb);
                                    m += 1;
                                }
                            }
                        }
                    }
                    seen[..m].iter().max_by(|x, y| x.1.cmp(&y.1).then(y.0.cmp(&x.0))).map(|s| s.2)
                })
                .collect()
        };
        let before = marked.len();
        let mut left = Vec::new();
        let moves: Vec<(usize, i32, u32)> = marked.iter().zip(&picks).filter_map(|(c, p)| p.map(|nb| (*c, cur[nb], src[nb]))).collect();
        for (c, p) in marked.iter().zip(&picks) {
            if p.is_none() {
                left.push(*c);
            }
        }
        for (c, l, s) in moves {
            (cur[c], src[c]) = (l, s);
        }
        if left.len() == before {
            // Grains with no neighbour to take them stay.
            break;
        }
        marked = left;
    }
}

/// Every labelled cell takes the place displaced by a smooth random field.
fn roughen(cur: &[i32], n: [usize; 3], h: [f64; 3], p: &RealismParams) -> Vec<u32> {
    const WAVES: usize = 16;
    let axes: Vec<usize> = (0..3).filter(|a| n[*a] > 1).collect();
    let size: [f64; 3] = std::array::from_fn(|a| n[a] as f64 * h[a]);
    let length = if p.roughness_length > 0.0 { p.roughness_length } else { 4.0 * p.roughness };
    let mut r = Rng::new(p.seed ^ 0x0A0B_0C0D_1234);
    let tau = std::f64::consts::TAU;
    let waves: Vec<([f64; 3], [f64; 3])> = (0..WAVES)
        .map(|_| {
            let k: [f64; 3] = std::array::from_fn(|a| {
                if !axes.contains(&a) {
                    return 0.0;
                }
                let k = r.std_normal() / length;
                if p.periodic[a] {
                    // A whole number of waves across a periodic box.
                    tau * (k * size[a] / tau).round() / size[a]
                } else {
                    k
                }
            });
            (k, std::array::from_fn(|_| r.uniform(0.0, tau)))
        })
        .collect();
    let amp = p.roughness * (2.0 / WAVES as f64).sqrt() / (axes.len().max(1) as f64).sqrt();
    (0..cur.len())
        .into_par_iter()
        .map(|c| {
            if cur[c] <= 0 {
                return c as u32;
            }
            let ijk = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
            let x: [f64; 3] = std::array::from_fn(|a| (ijk[a] as f64 + 0.5) * h[a]);
            let mut u = [0.0; 3];
            for (k, phase) in &waves {
                let kx = k[0] * x[0] + k[1] * x[1] + k[2] * x[2];
                for &a in &axes {
                    u[a] += (kx + phase[a]).cos();
                }
            }
            let mut t = [0usize; 3];
            for a in 0..3 {
                let q = ((x[a] + amp * u[a]) / h[a]).floor() as i64;
                t[a] = if p.periodic[a] { q.rem_euclid(n[a] as i64) as usize } else { q.clamp(0, n[a] as i64 - 1) as usize };
            }
            let target = (t[2] * n[1] + t[1]) * n[0] + t[0];
            if cur[target] > 0 {
                target as u32
            } else {
                c as u32
            }
        })
        .collect()
}

/// For every cell of a labelled grid (`ids`, 0 or less: no grain; `n` cells of size `h`), the
/// cell it takes its label and data from after roughening, coarsening and relaxation.
pub fn realism_map(ids: &[i32], n: [usize; 3], h: [f64; 3], p: &RealismParams) -> Vec<u32> {
    let mut src: Vec<u32> = (0..ids.len() as u32).collect();
    if p.is_off() {
        return src;
    }
    let mut cur = ids.to_vec();
    if p.roughness > 0.0 {
        src = roughen(&cur, n, h, p);
        cur = src.iter().map(|s| ids[*s as usize]).collect();
    }
    if p.coarsen > 0.0 {
        coarsen(&mut cur, &mut src, n, p.periodic, p.coarsen);
    }
    let k = Kernel::new(p.width, n, h);
    for _ in 0..p.relax {
        let (c, s) = sweep(&cur, &src, n, p.periodic, &k);
        let same = c == cur;
        (cur, src) = (c, s);
        if same {
            break;
        }
    }
    src
}

/// Realistic Grains: the labelled volume with its boundaries roughened, its smallest grains
/// removed and its boundaries relaxed; every cell array follows the cells, the grains left
/// are numbered from 1 in their old order and the per-feature field arrays keep their rows.
pub fn realistic_grains(ds: &DataSet, ids_name: &str, p: &RealismParams) -> Result<DataSet> {
    let img = image(ds)?;
    let ids = cell_i32(img, ids_name)?;
    if p.is_off() {
        return Ok(ds.clone());
    }
    let v = Vox::of(img);
    let src = realism_map(&ids, v.n, v.sp, p);
    let idx: Vec<usize> = src.iter().map(|s| *s as usize).collect();
    let mut out = img.clone();
    out.cell_data = img.cell_data.gather(&idx);
    let nf = rows(&ids);
    let mut present = vec![false; nf];
    present[0] = true;
    for s in &idx {
        if ids[*s] > 0 {
            present[ids[*s] as usize] = true;
        }
    }
    let kept: Vec<usize> = (0..nf).filter(|f| present[*f]).collect();
    let mut new_id = vec![0i32; nf];
    for (k, f) in kept.iter().enumerate() {
        new_id[*f] = k as i32;
    }
    out.cell_data.insert(Array::new(
        ids_name,
        1,
        ArrayData::I32(idx.iter().map(|s| if ids[*s] > 0 { new_id[ids[*s] as usize] } else { ids[*s].min(0) }).collect()),
    ));
    keep_feature_rows(&mut out, nf, &kept);
    Ok(DataSet::ImageData(out))
}

/// Fine equiaxed grains at the layer boundaries of a columnar structure: balls of diameter
/// `size` with centres just above every multiple of `layer` along z, `density` of them per
/// size² of cross-section, periodic across x and y. Labels (from 0, z slowest) are changed in
/// place; returns the number of new grains, numbered after `n_labels`. A ball takes the
/// cells nearer to its centre than to any other ball's.
pub fn stamp_equiaxed(labels: &mut [u32], n: [usize; 3], vox: f64, n_labels: usize, layer: f64, size: f64, density: f64, r: &mut dyn Draws) -> usize {
    let (lx, ly, lz) = (n[0] as f64 * vox, n[1] as f64 * vox, n[2] as f64 * vox);
    if layer <= 0.0 || size <= 0.0 || density <= 0.0 {
        return 0;
    }
    let per_band = ((density * lx * ly / (size * size)).round() as usize).max(1);
    let rad = 0.5 * size;
    let mut dist = vec![f64::INFINITY; labels.len()];
    let mut made = 0;
    let mut zl = layer;
    while zl < lz {
        for _ in 0..per_band {
            let c = [r.uniform(0.0, lx), r.uniform(0.0, ly), zl + r.uniform(0.0, rad)];
            let label = (n_labels + made) as u32;
            let mut used = false;
            let reach = (rad / vox).ceil() as i64 + 1;
            let centre = c.map(|x| (x / vox).floor() as i64);
            for dz in -reach..=reach {
                let k = centre[2] + dz;
                if k < 0 || k >= n[2] as i64 {
                    continue;
                }
                for dy in -reach..=reach {
                    let j = (centre[1] + dy).rem_euclid(n[1] as i64);
                    for dx in -reach..=reach {
                        let i = (centre[0] + dx).rem_euclid(n[0] as i64);
                        let mut ex = (i as f64 + 0.5) * vox - c[0];
                        let mut ey = (j as f64 + 0.5) * vox - c[1];
                        ex -= lx * (ex / lx).round();
                        ey -= ly * (ey / ly).round();
                        let ez = (k as f64 + 0.5) * vox - c[2];
                        let d2 = ex * ex + ey * ey + ez * ez;
                        let cell = (k as usize * n[1] + j as usize) * n[0] + i as usize;
                        if d2 <= rad * rad && d2 < dist[cell] {
                            dist[cell] = d2;
                            labels[cell] = label;
                            used = true;
                        }
                    }
                }
            }
            if used {
                made += 1;
            }
        }
        zl += layer;
    }
    made
}
