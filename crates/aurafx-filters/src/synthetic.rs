//! Synthetic microstructures: grains made by tessellation (Voronoi and Laguerre), columnar
//! grains of additive builds, orientation noise and jumbling, and the ensemble statistics of
//! a measured microstructure.

use crate::features::image;
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::{self as cx, Laue, Quat};
use aurafx_data::*;
use crate::Association;
use rayon::prelude::*;

/// A source of random numbers: uniform on [0, 1) and standard normal.
pub trait Draws {
    fn random(&mut self) -> f64;
    fn std_normal(&mut self) -> f64;
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.random()
    }
    fn normal(&mut self, loc: f64, scale: f64) -> f64 {
        loc + scale * self.std_normal()
    }
}

/// xorshift64* with Box-Muller normals; a fixed seed gives the same microstructure.
pub struct Rng {
    s: u64,
    spare: Option<f64>,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { s: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03 | 1, spare: None }
    }
}

impl Draws for Rng {
    fn random(&mut self) -> f64 {
        self.s ^= self.s >> 12;
        self.s ^= self.s << 25;
        self.s ^= self.s >> 27;
        (self.s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn std_normal(&mut self) -> f64 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let (u, v) = (self.random().max(1e-300), self.random());
        let r = (-2.0 * u.ln()).sqrt();
        self.spare = Some(r * (std::f64::consts::TAU * v).sin());
        r * (std::f64::consts::TAU * v).cos()
    }
}

/// Recorded draws played back in order (checks that each is of the kind asked for).
pub struct Replay {
    pub draws: Vec<(bool, f64)>,
    pub pos: usize,
}

impl Draws for Replay {
    fn random(&mut self) -> f64 {
        let (normal, v) = self.draws[self.pos];
        assert!(!normal, "draw {} is a normal value, a uniform one was asked for", self.pos);
        self.pos += 1;
        v
    }
    fn std_normal(&mut self) -> f64 {
        let (normal, v) = self.draws[self.pos];
        assert!(normal, "draw {} is a uniform value, a normal one was asked for", self.pos);
        self.pos += 1;
        v
    }
}

/// A uniformly random orientation (Shoemake).
pub fn random_quat(r: &mut dyn Draws) -> Quat {
    cx::random_quat(r.random(), r.random(), r.random())
}

// ------------------------------------------------------------------ tessellations

/// Labels (0-based, x fastest) of the power diagram of `seeds` with `weights` on a grid of
/// `n` cells of size `h` from the origin; periodic or not.
pub fn power_labels(n: [usize; 3], h: [f64; 3], seeds: &[[f64; 3]], weights: &[f64], periodic: bool) -> Vec<u32> {
    let size: [f64; 3] = std::array::from_fn(|a| n[a] as f64 * h[a]);
    // Buckets of seeds about one mean spacing wide.
    let vol = size[0] * size[1] * size[2];
    let spacing = (vol / seeds.len().max(1) as f64).cbrt();
    let nb: [usize; 3] = std::array::from_fn(|a| ((size[a] / spacing).floor() as usize).clamp(1, 64));
    let bw: [f64; 3] = std::array::from_fn(|a| size[a] / nb[a] as f64);
    let mut bucket: Vec<Vec<u32>> = vec![Vec::new(); nb[0] * nb[1] * nb[2]];
    for (i, s) in seeds.iter().enumerate() {
        let b: [usize; 3] = std::array::from_fn(|a| ((s[a] / bw[a]) as usize).min(nb[a] - 1));
        bucket[b[0] + nb[0] * (b[1] + nb[1] * b[2])].push(i as u32);
    }
    let wmax = weights.iter().cloned().fold(0.0, f64::max);
    let total = n[0] * n[1] * n[2];
    (0..total)
        .into_par_iter()
        .map(|c| {
            let ijk = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
            let p: [f64; 3] = std::array::from_fn(|a| (ijk[a] as f64 + 0.5) * h[a]);
            let pb: [i64; 3] = std::array::from_fn(|a| ((p[a] / bw[a]) as i64).min(nb[a] as i64 - 1));
            let dist = |s: &[f64; 3]| -> f64 {
                (0..3)
                    .map(|a| {
                        let mut d = p[a] - s[a];
                        if periodic {
                            d -= size[a] * (d / size[a]).round();
                        }
                        d * d
                    })
                    .sum()
            };
            let mut best = (f64::INFINITY, 0u32);
            let mut ring: i64 = 0;
            loop {
                // Search the shell of buckets at Chebyshev distance `ring`.
                for dz in -ring..=ring {
                    for dy in -ring..=ring {
                        for dx in -ring..=ring {
                            if dx.abs().max(dy.abs()).max(dz.abs()) != ring {
                                continue;
                            }
                            let q = [pb[0] + dx, pb[1] + dy, pb[2] + dz];
                            let mut idx = [0usize; 3];
                            let mut ok = true;
                            for a in 0..3 {
                                if q[a] < 0 || q[a] >= nb[a] as i64 {
                                    if periodic {
                                        idx[a] = q[a].rem_euclid(nb[a] as i64) as usize;
                                    } else {
                                        ok = false;
                                    }
                                } else {
                                    idx[a] = q[a] as usize;
                                }
                            }
                            if !ok {
                                continue;
                            }
                            for &i in &bucket[idx[0] + nb[0] * (idx[1] + nb[1] * idx[2])] {
                                let d = dist(&seeds[i as usize]) - weights[i as usize];
                                if d < best.0 || (d == best.0 && i < best.1) {
                                    best = (d, i);
                                }
                            }
                        }
                    }
                }
                // Every seed beyond this shell is at least `ring` bucket widths away.
                let reach = ring as f64 * bw.iter().cloned().fold(f64::INFINITY, f64::min);
                let covers = (0..3).all(|a| (2 * ring + 1) as usize >= nb[a]);
                if (best.0.is_finite() && reach * reach - wmax > best.0) || covers {
                    break;
                }
                ring += 1;
            }
            best.1
        })
        .collect()
}

/// Labels renumbered from 1 in order of first appearance; the old label of each new one.
fn compact(labels: &[u32]) -> (Vec<i32>, Vec<u32>) {
    let mut map = std::collections::HashMap::new();
    let mut order = Vec::new();
    let ids = labels
        .iter()
        .map(|l| {
            *map.entry(*l).or_insert_with(|| {
                order.push(*l);
                order.len() as i32
            })
        })
        .collect();
    (ids, order)
}

/// Orientations of synthetic grains.
pub const ORIENT_RANDOM: &str = "Random";
pub const ORIENT_FIBER100: &str = "<100> along Z";
pub const ORIENT_FIBER111: &str = "<111> along Z";

/// One orientation per grain: random, or a fibre (a crystal direction along sample Z within
/// a Gaussian spread of `spread` radians, random about Z).
fn grain_orientations(n: usize, kind: &str, spread: f64, r: &mut dyn Draws) -> Vec<Quat> {
    (0..n)
        .map(|_| {
            if kind == ORIENT_RANDOM {
                return random_quat(r);
            }
            // Crystal direction c along sample z: g z = c, then random about z, tilted.
            let c: [f64; 3] = if kind == ORIENT_FIBER111 { [1.0 / 3f64.sqrt(); 3] } else { [0.0, 0.0, 1.0] };
            // Rotation taking z to c (active, crystal frame), as the orientation g = R^T? We
            // build g with g z = c: g = R(z -> c).
            let z = [0.0, 0.0, 1.0];
            let axis = [z[1] * c[2] - z[2] * c[1], z[2] * c[0] - z[0] * c[2], z[0] * c[1] - z[1] * c[0]];
            let s = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
            let base = if s < 1e-12 { cx::IDENTITY } else { cx::axis_angle_to_quat(axis.map(|x| x / s), s.atan2(c[2])) };
            let about_z = cx::axis_angle_to_quat([0.0, 0.0, 1.0], r.uniform(0.0, std::f64::consts::TAU));
            let tilt_axis = {
                let a = r.uniform(0.0, std::f64::consts::TAU);
                [a.cos(), a.sin(), 0.0]
            };
            let tilt = cx::axis_angle_to_quat(tilt_axis, (spread * r.std_normal()).abs());
            // g = base * about_z * tilt (sample-frame rotations applied first).
            cx::normalize(cx::mul(base, cx::mul(about_z, tilt)))
        })
        .collect()
}

/// A microstructure dataset: cells of `h` on `n`, FeatureIds, Phases (1), EulerAngles, and
/// the class of phase 1 as CrystalStructures.
fn microstructure(n: [usize; 3], h: [f64; 3], ids: Vec<i32>, orients: &[Quat], laue: Laue) -> ImageData {
    let mut img = ImageData { extent: [0, n[0] as i64, 0, n[1] as i64, 0, n[2] as i64], origin: [0.0; 3], spacing: h, ..Default::default() };
    let eul: Vec<f32> = ids.iter().flat_map(|i| cx::quat_to_euler(orients[(*i - 1) as usize]).map(|x| x as f32)).collect();
    let nc = ids.len();
    img.cell_data.insert(Array::new("FeatureIds", 1, ArrayData::I32(ids)));
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(vec![1; nc])));
    img.cell_data.insert(Array::new("EulerAngles", 3, ArrayData::F32(eul)));
    img.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![Laue::UNKNOWN_CODE, laue.code()])));
    img
}

pub struct TessellationParams {
    pub cells: [usize; 3],
    pub spacing: f64,
    pub grains: usize,
    /// Log-standard deviation of the seed radii (0: Voronoi).
    pub size_sigma: f64,
    pub periodic: bool,
    pub orientations: String,
    pub spread_deg: f64,
    pub structure: String,
    pub seed: u64,
}

/// Voronoi / Laguerre Grains: seeds placed at random, each cell of the grid given to the
/// seed of least power distance |x - s|² - w (w = 0: Voronoi; w from lognormal radii:
/// Laguerre, grains of different sizes).
pub fn tessellation(p: &TessellationParams) -> Result<DataSet> {
    let n = p.cells.map(|c| c.max(1));
    if n[0] * n[1] * n[2] > 100_000_000 {
        bail!("more than 100 million cells");
    }
    let h = [p.spacing; 3];
    let size: [f64; 3] = std::array::from_fn(|a| n[a] as f64 * h[a]);
    let laue = Laue::from_label(&p.structure).ok_or_else(|| anyhow!("unknown crystal structure {}", p.structure))?;
    let mut r = Rng::new(p.seed);
    let ng = p.grains.max(1);
    let seeds: Vec<[f64; 3]> = (0..ng).map(|_| std::array::from_fn(|a| r.uniform(0.0, size[a]))).collect();
    let spacing = (size[0] * size[1] * size[2] / ng as f64).cbrt();
    let weights: Vec<f64> = (0..ng)
        .map(|_| {
            if p.size_sigma <= 0.0 {
                0.0
            } else {
                let rad = 0.5 * spacing * (p.size_sigma * r.std_normal().clamp(-3.0, 3.0)).exp();
                rad * rad
            }
        })
        .collect();
    let labels = power_labels(n, h, &seeds, &weights, p.periodic);
    let (ids, order) = compact(&labels);
    let orients = grain_orientations(order.len(), &p.orientations, p.spread_deg.to_radians(), &mut r);
    let mut img = microstructure(n, h, ids, &orients, laue);
    let mut sx = vec![0.0; order.len() + 1];
    let mut sy = vec![0.0; order.len() + 1];
    let mut sz = vec![0.0; order.len() + 1];
    for (k, o) in order.iter().enumerate() {
        let s = seeds[*o as usize];
        (sx[k + 1], sy[k + 1], sz[k + 1]) = (s[0], s[1], s[2]);
    }
    let seeds_arr: Vec<f64> = (0..=order.len()).flat_map(|k| [sx[k], sy[k], sz[k]]).collect();
    img.field_data.insert(Array::new("Seeds", 3, ArrayData::F64(seeds_arr)));
    Ok(DataSet::ImageData(img))
}

// ------------------------------------------------------------------ columnar grains

#[derive(Clone, Debug)]
pub struct ColumnParams {
    pub size: [f64; 3],
    pub voxel: f64,
    pub width: f64,
    pub height: f64,
    pub height_sigma: f64,
    pub tilt_sd_deg: f64,
    pub width_sigma: f64,
    pub width_height_exponent: f64,
}

/// Columns of an additive build: labels (z slowest, then y, then x; from 0), the grid size
/// and per grain (base, top, tilt in degrees, originating in the build).
pub struct Columns {
    pub n: [usize; 3],
    pub labels: Vec<u32>,
    pub base: Vec<f64>,
    pub top: Vec<f64>,
    pub tilt: Vec<f64>,
    pub originating: Vec<bool>,
}

/// Columnar grains growing from the base plate: `n_active = A / w²` columns are alive at
/// every height; each grows to a lognormal height (median `height`, log-sigma
/// `height_sigma`) or until it is overgrown, and is then replaced at that height by a grain
/// starting at a random lateral position. Each slice is the periodic power diagram of the
/// columns alive there, centred on the column's tilted axis, with weights that vary the
/// cross-sections (or, with `width_sigma` / `width_height_exponent`, lognormal widths that
/// grow with the column's height). Grains are connected stacks of slices.
pub fn columns(p: &ColumnParams, r: &mut dyn Draws) -> Result<Columns> {
    let vox = p.voxel;
    let nn: [usize; 3] = std::array::from_fn(|a| ((p.size[a] / vox).round() as usize).max(4));
    let (lx, ly, lz) = (nn[0] as f64 * vox, nn[1] as f64 * vox, nn[2] as f64 * vox);
    let w = p.width;
    let n_active = ((lx * ly / (w * w)).round() as usize).max(3);
    let (mut x0, mut y0, mut zb, mut zt, mut tx, mut ty, mut wt, mut tilt, mut appeared, mut ended) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    #[allow(clippy::too_many_arguments)]
    fn spawn(
        z_base: f64,
        p: &ColumnParams,
        lx: f64,
        ly: f64,
        r: &mut dyn Draws,
        v: (&mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>, &mut Vec<bool>, &mut Vec<bool>),
    ) {
        let (x0, y0, zb, zt, tx, ty, wt, tilt, appeared, ended) = v;
        appeared.push(false);
        ended.push(false);
        x0.push(r.uniform(0.0, lx));
        y0.push(r.uniform(0.0, ly));
        zb.push(z_base);
        zt.push(z_base + p.height * (p.height_sigma * r.std_normal()).exp());
        let t = r.normal(0.0, p.tilt_sd_deg).abs();
        let a = r.uniform(0.0, 2.0 * std::f64::consts::PI);
        tilt.push(t);
        tx.push(t.to_radians().tan() * a.cos());
        ty.push(t.to_radians().tan() * a.sin());
        if p.width_sigma > 0.0 || p.width_height_exponent != 0.0 {
            let h = (zt.last().unwrap() - z_base) / p.height;
            wt.push(0.5 * p.width * (p.width_sigma * r.std_normal().clamp(-2.5, 2.5)).exp() * h.min(20.0).powf(p.width_height_exponent));
        } else {
            wt.push(0.35 * p.width * r.random());
        }
    }
    macro_rules! sp {
        ($z:expr) => {
            spawn($z, p, lx, ly, r, (&mut x0, &mut y0, &mut zb, &mut zt, &mut tx, &mut ty, &mut wt, &mut tilt, &mut appeared, &mut ended))
        };
    }
    for _ in 0..n_active {
        sp!(0.0);
    }
    let (nx, ny, nz) = (nn[0], nn[1], nn[2]);
    let mut labels = vec![u32::MAX; nx * ny * nz];
    for k in 0..nz {
        let z = (k as f64 + 0.5) * vox;
        let mut i = 0;
        while i < zt.len() {
            if !ended[i] && zt[i] <= z {
                ended[i] = true;
                if zt[i] < lz {
                    let zi = zt[i];
                    sp!(zi);
                }
            }
            i += 1;
        }
        let act: Vec<usize> = (0..zb.len()).filter(|i| zb[*i] <= z && !ended[*i]).collect();
        let cx: Vec<f64> = act.iter().map(|i| rem(x0[*i] + (z - zb[*i]) * tx[*i], lx)).collect();
        let cy: Vec<f64> = act.iter().map(|i| rem(y0[*i] + (z - zb[*i]) * ty[*i], ly)).collect();
        let w2: Vec<f64> = act.iter().map(|i| wt[*i] * wt[*i]).collect();
        let slice: Vec<u32> = (0..nx * ny)
            .into_par_iter()
            .map(|c| {
                let (px, py) = (((c % nx) as f64 + 0.5) * vox, ((c / nx) as f64 + 0.5) * vox);
                let mut best = (f64::INFINITY, 0usize);
                for j in 0..act.len() {
                    let mut dx = px - cx[j];
                    let mut dy = py - cy[j];
                    dx -= lx * (dx / lx).round_ties_even();
                    dy -= ly * (dy / ly).round_ties_even();
                    let d2 = dx * dx + dy * dy - w2[j];
                    if d2 < best.0 {
                        best = (d2, j);
                    }
                }
                act[best.1] as u32
            })
            .collect();
        let mut on = vec![false; zt.len()];
        for l in &slice {
            on[*l as usize] = true;
        }
        labels[k * nx * ny..(k + 1) * nx * ny].copy_from_slice(&slice);
        for &i in &act {
            if !on[i] && appeared[i] {
                zt[i] = z;
                ended[i] = true;
                sp!(z);
            }
        }
        for (i, o) in on.iter().enumerate() {
            if *o {
                appeared[i] = true;
            }
        }
    }
    // Renumber the grains present, in order of their number.
    let mut present: Vec<u32> = labels.clone();
    present.sort_unstable();
    present.dedup();
    let mut remap = vec![u32::MAX; zb.len()];
    for (k, l) in present.iter().enumerate() {
        remap[*l as usize] = k as u32;
    }
    let labels: Vec<u32> = labels.iter().map(|l| remap[*l as usize]).collect();
    let ng = present.len();
    let mut zmin = vec![nz; ng];
    let mut zmax = vec![0usize; ng];
    for k in 0..nz {
        for l in &labels[k * nx * ny..(k + 1) * nx * ny] {
            zmin[*l as usize] = zmin[*l as usize].min(k);
            zmax[*l as usize] = zmax[*l as usize].max(k);
        }
    }
    Ok(Columns {
        n: nn,
        labels,
        base: zmin.iter().map(|k| *k as f64 * vox).collect(),
        top: zmax.iter().map(|k| (*k + 1) as f64 * vox).collect(),
        tilt: present.iter().map(|l| tilt[*l as usize]).collect(),
        originating: present.iter().map(|l| zb[*l as usize] > 0.0).collect(),
    })
}

/// x mod m with the sign of m (as Python's %).
fn rem(x: f64, m: f64) -> f64 {
    let r = x % m;
    if r != 0.0 && (r < 0.0) != (m < 0.0) {
        r + m
    } else {
        r
    }
}

/// Columnar Grains as a dataset (z the build direction).
pub fn columnar_grains(p: &ColumnParams, orientations: &str, spread_deg: f64, structure: &str, seed: u64) -> Result<DataSet> {
    let laue = Laue::from_label(structure).ok_or_else(|| anyhow!("unknown crystal structure {structure}"))?;
    let cells = p.size.iter().map(|s| (s / p.voxel).round().max(4.0)).product::<f64>();
    if cells > 100e6 {
        bail!("more than 100 million cells: use larger voxels");
    }
    let mut r = Rng::new(seed);
    let c = columns(p, &mut r)?;
    let ng = c.base.len();
    let orients = grain_orientations(ng, orientations, spread_deg.to_radians(), &mut r);
    let ids: Vec<i32> = c.labels.iter().map(|l| *l as i32 + 1).collect();
    let mut img = microstructure(c.n, [p.voxel; 3], ids, &orients, laue);
    let with0 = |v: &[f64]| -> Vec<f64> { std::iter::once(0.0).chain(v.iter().copied()).collect() };
    img.field_data.insert(Array::scalars_f64("GrainBase", with0(&c.base)));
    img.field_data.insert(Array::scalars_f64("GrainTop", with0(&c.top)));
    img.field_data.insert(Array::scalars_f64("GrainTilt", with0(&c.tilt)));
    img.field_data.insert(Array::new("Originating", 1, ArrayData::U8(std::iter::once(0).chain(c.originating.iter().map(|o| *o as u8)).collect())));
    Ok(DataSet::ImageData(img))
}

// ------------------------------------------------------------------ orientation changes

fn orientation_target(ds: &DataSet, name: &str) -> Result<(Association, Array)> {
    let (assoc, a) = crate::crystal::orientation_array(ds, if name.is_empty() { None } else { Some((Association::Cells, name.to_string())) })?;
    Ok((assoc, a.clone()))
}

fn encode_like(a: &Array, qs: &[Quat]) -> Result<Array> {
    let repr = crate::crystal::Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
    let vals: Vec<f64> = qs.iter().flat_map(|q| repr.encode(*q)).collect();
    Ok(Array::new(&a.name, a.ncomp, ArrayData::F64(vals)))
}

fn replace(ds: &DataSet, assoc: Association, arr: Array) -> Result<DataSet> {
    let mut out = ds.clone();
    let attrs = match assoc {
        Association::Cells => out.cell_data_mut(),
        Association::Points => out.point_data_mut(),
    }
    .ok_or_else(|| anyhow!("this data cannot hold arrays"))?;
    attrs.insert(arr);
    Ok(out)
}

/// A random rotation of angle |N(0, sigma)| about a random axis.
fn small_rotation(sigma: f64, r: &mut dyn Draws) -> Quat {
    let z = r.uniform(-1.0, 1.0);
    let phi = r.uniform(0.0, std::f64::consts::TAU);
    let s = (1.0 - z * z).sqrt();
    cx::axis_angle_to_quat([s * phi.cos(), s * phi.sin(), z], (sigma * r.std_normal()).abs())
}

/// Add Orientation Noise: every point's orientation (or every feature's, all its points
/// alike) turned by a random rotation whose angle is |N(0, sigma)|.
pub fn orientation_noise(ds: &DataSet, orientations: &str, sigma_deg: f64, per_feature: bool, ids_name: &str, seed: u64) -> Result<DataSet> {
    let (assoc, a) = orientation_target(ds, orientations)?;
    let repr = crate::crystal::Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
    let mut r = Rng::new(seed);
    let sigma = sigma_deg.to_radians();
    let n = a.len();
    let qs: Vec<Quat> = if per_feature {
        let ids = feature_ids(ds, ids_name, assoc, n)?;
        let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
        let rot: Vec<Quat> = (0..nf).map(|_| small_rotation(sigma, &mut r)).collect();
        (0..n).map(|i| if ids[i] > 0 { cx::normalize(cx::mul(rot[ids[i] as usize], repr.decode_values(&a, i))) } else { repr.decode_values(&a, i) }).collect()
    } else {
        (0..n).map(|i| cx::normalize(cx::mul(small_rotation(sigma, &mut r), repr.decode_values(&a, i)))).collect()
    };
    replace(ds, assoc, encode_like(&a, &qs)?)
}

fn feature_ids(ds: &DataSet, name: &str, assoc: Association, n: usize) -> Result<Vec<i32>> {
    let attrs = match assoc {
        Association::Cells => ds.cell_data(),
        Association::Points => ds.point_data(),
    };
    let a = attrs.and_then(|x| x.get(name)).ok_or_else(|| anyhow!("no array {name} (run Segment Features first)"))?;
    if a.len() != n {
        bail!("{name} and the orientations have different lengths");
    }
    Ok((0..n).map(|i| a.get(i, 0) as i32).collect())
}

/// Jumble Orientations: the features swap orientations at random (each feature takes the
/// orientation of its first point from another feature; every point of a feature gets it).
pub fn jumble_orientations(ds: &DataSet, orientations: &str, ids_name: &str, seed: u64) -> Result<DataSet> {
    let (assoc, a) = orientation_target(ds, orientations)?;
    let repr = crate::crystal::Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
    let n = a.len();
    let ids = feature_ids(ds, ids_name, assoc, n)?;
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    let mut first: Vec<Option<Quat>> = vec![None; nf];
    for i in 0..n {
        if ids[i] > 0 && first[ids[i] as usize].is_none() {
            first[ids[i] as usize] = Some(repr.decode_values(&a, i));
        }
    }
    let present: Vec<usize> = (1..nf).filter(|f| first[*f].is_some()).collect();
    let mut perm = present.clone();
    let mut r = Rng::new(seed);
    for k in (1..perm.len()).rev() {
        let j = (r.random() * (k + 1) as f64) as usize;
        perm.swap(k, j.min(k));
    }
    let mut new = vec![cx::IDENTITY; nf];
    for (f, g) in present.iter().zip(&perm) {
        new[*f] = first[*g].unwrap();
    }
    let qs: Vec<Quat> = (0..n).map(|i| if ids[i] > 0 { new[ids[i] as usize] } else { repr.decode_values(&a, i) }).collect();
    replace(ds, assoc, encode_like(&a, &qs)?)
}

// ------------------------------------------------------------------ ensemble statistics

/// Moments fit of a beta distribution on (0, 1): (alpha, beta).
pub fn beta_fit(x: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    if x.len() < 2 {
        return (f64::NAN, f64::NAN);
    }
    let m = x.iter().sum::<f64>() / n;
    let v = x.iter().map(|a| (a - m).powi(2)).sum::<f64>() / (n - 1.0);
    if v <= 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let c = m * (1.0 - m) / v - 1.0;
    (m * c, (1.0 - m) * c)
}

/// Fit of a lognormal: (mu, sigma) of the logarithms.
pub fn lognormal_fit(x: &[f64]) -> (f64, f64) {
    let l: Vec<f64> = x.iter().filter(|v| **v > 0.0).map(|v| v.ln()).collect();
    let n = l.len() as f64;
    if l.len() < 2 {
        return (f64::NAN, f64::NAN);
    }
    let m = l.iter().sum::<f64>() / n;
    (m, (l.iter().map(|a| (a - m).powi(2)).sum::<f64>() / (n - 1.0)).sqrt())
}

/// Generate Ensemble Statistics: size (lognormal fit of the equivalent diameters), shape
/// (beta fits of b/a and c/b), neighbour counts (lognormal fit) and the feature count of a
/// segmented microstructure, as a table of names and values (also as field data). Features
/// touching the outside are left out unless `include_surface`.
pub fn ensemble_statistics(ds: &DataSet, ids_name: &str, include_surface: bool) -> Result<DataSet> {
    let _ = image(ds)?;
    let mut d = crate::features::feature_sizes(ds, ids_name)?;
    d = crate::features::feature_centroids(&d, ids_name)?;
    d = crate::features::feature_shapes(&d, ids_name)?;
    d = crate::features::feature_neighbors(&d, ids_name)?;
    let fd = d.field_data();
    let get = |n: &str| fd.get(n).ok_or_else(|| anyhow!("no {n}"));
    let (esd, ar, nn, surf, cnt) = (get("EquivalentDiameters")?, get("AspectRatios")?, get("NumNeighbors")?, get("SurfaceFeatures")?, get("NumElements")?);
    let keep: Vec<usize> = (1..esd.len()).filter(|f| cnt.get(*f, 0) > 0.0 && (include_surface || surf.get(*f, 0) == 0.0)).collect();
    if keep.len() < 2 {
        bail!("fewer than two features to describe (try including the features on the surface)");
    }
    let sizes: Vec<f64> = keep.iter().map(|f| esd.get(*f, 0)).collect();
    let ba: Vec<f64> = keep.iter().map(|f| ar.get(*f, 0)).filter(|v| *v > 0.0 && *v < 1.0).collect();
    let cb: Vec<f64> = keep.iter().map(|f| ar.get(*f, 1)).filter(|v| *v > 0.0 && *v < 1.0).collect();
    let nb: Vec<f64> = keep.iter().map(|f| nn.get(*f, 0)).collect();
    let (mu, sigma) = lognormal_fit(&sizes);
    let (a1, b1) = beta_fit(&ba);
    let (a2, b2) = beta_fit(&cb);
    let (nmu, nsig) = lognormal_fit(&nb);
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let rows: Vec<(&str, f64)> = vec![
        ("Features", keep.len() as f64),
        ("ESD mu", mu),
        ("ESD sigma", sigma),
        ("ESD mean", mean(&sizes)),
        ("ESD min", sizes.iter().cloned().fold(f64::INFINITY, f64::min)),
        ("ESD max", sizes.iter().cloned().fold(0.0, f64::max)),
        ("b/a alpha", a1),
        ("b/a beta", b1),
        ("b/a mean", mean(&ba)),
        ("c/b alpha", a2),
        ("c/b beta", b2),
        ("c/b mean", mean(&cb)),
        ("Neighbors mu", nmu),
        ("Neighbors sigma", nsig),
        ("Neighbors mean", mean(&nb)),
    ];
    let mut t = Table::default();
    t.columns.insert(Array::new("Statistic", 1, ArrayData::String(rows.iter().map(|r| r.0.to_string()).collect())));
    t.columns.insert(Array::scalars_f64("Value", rows.iter().map(|r| r.1).collect()));
    for (k, v) in &rows {
        t.field_data.insert(Array::scalars_f64(*k, vec![*v]));
    }
    Ok(DataSet::Table(t))
}
