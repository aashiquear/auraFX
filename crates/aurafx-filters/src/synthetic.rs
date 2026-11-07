//! Synthetic microstructures: grains made by tessellation (Voronoi and Laguerre), columnar
//! grains of additive builds, orientation noise and jumbling, and the ensemble statistics of
//! a measured microstructure.

use crate::features::image;
use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::{self as cx, Laue, Quat};
use aurafx_data::*;
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
    let (mut x0, mut y0, mut zb, mut zt, mut tx, mut ty, mut wt, mut tilt, mut appeared, mut ended) =
        (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    #[allow(clippy::too_many_arguments)]
    fn spawn(
        z_base: f64,
        p: &ColumnParams,
        lx: f64,
        ly: f64,
        r: &mut dyn Draws,
        v: (
            &mut Vec<f64>,
            &mut Vec<f64>,
            &mut Vec<f64>,
            &mut Vec<f64>,
            &mut Vec<f64>,
            &mut Vec<f64>,
            &mut Vec<f64>,
            &mut Vec<f64>,
            &mut Vec<bool>,
            &mut Vec<bool>,
        ),
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
/// (beta fits of the aspect ratios b/a and c/a), neighbour counts (lognormal fit) and the feature count of a
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
    let ca: Vec<f64> = keep.iter().map(|f| ar.get(*f, 1)).filter(|v| *v > 0.0 && *v < 1.0).collect();
    let nb: Vec<f64> = keep.iter().map(|f| nn.get(*f, 0)).collect();
    let (mu, sigma) = lognormal_fit(&sizes);
    let (a1, b1) = beta_fit(&ba);
    let (a2, b2) = beta_fit(&ca);
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
        ("c/a alpha", a2),
        ("c/a beta", b2),
        ("c/a mean", mean(&ca)),
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

// ------------------------------------------------------------------ statistics-driven packing

pub const SHAPE_ELLIPSOID: &str = "Ellipsoid";
pub const SHAPE_SUPER: &str = "Superellipsoid";
pub const SHAPE_CUBOCT: &str = "Cube-octahedron";
pub const SHAPE_CYLINDER: &str = "Cylinder";

/// ln Γ(x) (Lanczos, x > 0).
fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        return (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = G[0];
    let t = x + 7.5;
    for (i, g) in G.iter().enumerate().skip(1) {
        a += g / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// A Gamma(k, 1) draw (Marsaglia and Tsang).
fn gamma_draw(k: f64, r: &mut dyn Draws) -> f64 {
    if k < 1.0 {
        let u = r.random().max(1e-300);
        return gamma_draw(k + 1.0, r) * u.powf(1.0 / k);
    }
    let d = k - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = r.std_normal();
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 {
            continue;
        }
        let u = r.random().max(1e-300);
        if u.ln() < 0.5 * x * x + d - d * v + d * v.ln() {
            return d * v;
        }
    }
}

/// A Beta(a, b) draw.
fn beta_draw(a: f64, b: f64, r: &mut dyn Draws) -> f64 {
    let x = gamma_draw(a, r);
    let y = gamma_draw(b, r);
    x / (x + y)
}

/// The shape of packed grains.
#[derive(Clone, Copy)]
enum GrainShape {
    Ellipsoid,
    Super(f64),
    CubOct,
    Cylinder,
}

impl GrainShape {
    fn of(name: &str, exponent: f64) -> GrainShape {
        match name {
            SHAPE_SUPER => GrainShape::Super(exponent.max(1.0)),
            SHAPE_CUBOCT => GrainShape::CubOct,
            SHAPE_CYLINDER => GrainShape::Cylinder,
            _ => GrainShape::Ellipsoid,
        }
    }
    /// 1 on the surface of the unit shape, below 1 inside.
    fn norm(self, u: [f64; 3]) -> f64 {
        let a = u.map(f64::abs);
        match self {
            GrainShape::Ellipsoid => (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt(),
            GrainShape::Super(n) => (a[0].powf(n) + a[1].powf(n) + a[2].powf(n)).powf(1.0 / n),
            GrainShape::CubOct => a[0].max(a[1]).max(a[2]).max((a[0] + a[1] + a[2]) / 1.5),
            GrainShape::Cylinder => (a[0] * a[0] + a[1] * a[1]).sqrt().max(a[2]),
        }
    }
    /// Volume of the unit shape.
    fn volume(self) -> f64 {
        match self {
            GrainShape::Ellipsoid => 4.0 / 3.0 * std::f64::consts::PI,
            GrainShape::Super(n) => 8.0 * (3.0 * ln_gamma(1.0 + 1.0 / n) - ln_gamma(1.0 + 3.0 / n)).exp(),
            GrainShape::CubOct => 8.0 - 1.0 / 6.0,
            GrainShape::Cylinder => 2.0 * std::f64::consts::PI,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PackParams {
    pub cells: [usize; 3],
    pub spacing: f64,
    pub esd_mu: f64,
    pub esd_sigma: f64,
    /// Sizes are drawn between mu - min_cut sigma and mu + max_cut sigma (of the logarithms).
    pub min_cut: f64,
    pub max_cut: f64,
    pub ba: (f64, f64),
    /// c/a, at most b/a.
    pub ca: (f64, f64),
    pub shape: String,
    pub exponent: f64,
    /// Grain axes along x, y, z (else turned at random).
    pub aligned: bool,
    /// Volume fraction of the grains; the rest is a matrix phase.
    pub fraction: f64,
    pub periodic: bool,
    pub orientations: String,
    pub spread_deg: f64,
    pub structure: String,
    pub seed: u64,
    /// Placement moves per grain.
    pub moves: usize,
    /// Repack with corrected goals until the measured size and aspect ratios match them.
    pub calibrate: bool,
}

struct Packed {
    c: [f64; 3],
    /// Grain axes (rows: the directions of a, b, c in the sample).
    rot: [[f64; 3]; 3],
    ax: [f64; 3],
}

impl Packed {
    fn reach(&self) -> f64 {
        self.ax.iter().cloned().fold(0.0, f64::max) * 1.8
    }
}

/// Grains drawn from the goal statistics until their volume reaches the goal fraction.
fn draw_grains(p: &PackParams, shape: GrainShape, size: [f64; 3], goal: f64, r: &mut dyn Draws) -> Result<Vec<Packed>> {
    let unit = shape.volume();
    let mut out = Vec::new();
    let mut vol = 0.0;
    while vol < goal {
        if out.len() > 2_000_000 {
            bail!("more than 2 million grains: make the grains larger or the box smaller");
        }
        let z = loop {
            let z = r.std_normal();
            if z >= -p.min_cut && z <= p.max_cut {
                break z;
            }
        };
        let esd = (p.esd_mu + p.esd_sigma * z).exp();
        let ba = beta_draw(p.ba.0.max(1e-3), p.ba.1.max(1e-3), r).clamp(0.05, 1.0);
        let ca = beta_draw(p.ca.0.max(1e-3), p.ca.1.max(1e-3), r).clamp(0.05, 1.0).min(ba);
        // Volume of the grain equals the sphere of diameter esd: unit * a b c = pi/6 esd^3.
        let v = std::f64::consts::PI / 6.0 * esd.powi(3);
        let a = (v / (unit * ba * ca)).cbrt();
        let ax = [a, a * ba, a * ca];
        let rot = if p.aligned { [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] } else { cx::transpose(&cx::quat_to_matrix(random_quat(r))) };
        let c = std::array::from_fn(|k| r.uniform(0.0, size[k]));
        out.push(Packed { c, rot, ax });
        vol += v;
    }
    Ok(out)
}

struct Lattice {
    n: [usize; 3],
    h: f64,
    periodic: bool,
}

impl Lattice {
    /// Cells whose centres lie inside the grain.
    fn cells_of(&self, g: &Packed, shape: GrainShape) -> Vec<usize> {
        let r = g.reach();
        let mut out = Vec::new();
        let lo: [i64; 3] = std::array::from_fn(|k| ((g.c[k] - r) / self.h).floor() as i64);
        let hi: [i64; 3] = std::array::from_fn(|k| ((g.c[k] + r) / self.h).ceil() as i64);
        for k in lo[2]..=hi[2] {
            for j in lo[1]..=hi[1] {
                for i in lo[0]..=hi[0] {
                    let q = [i, j, k];
                    let mut idx = [0usize; 3];
                    let mut ok = true;
                    for a in 0..3 {
                        if q[a] < 0 || q[a] >= self.n[a] as i64 {
                            if self.periodic {
                                idx[a] = q[a].rem_euclid(self.n[a] as i64) as usize;
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
                    let d: [f64; 3] = std::array::from_fn(|a| (q[a] as f64 + 0.5) * self.h - g.c[a]);
                    let u: [f64; 3] = std::array::from_fn(|a| (g.rot[a][0] * d[0] + g.rot[a][1] * d[1] + g.rot[a][2] * d[2]) / g.ax[a]);
                    if shape.norm(u) <= 1.0 {
                        out.push(idx[0] + self.n[0] * (idx[1] + self.n[1] * idx[2]));
                    }
                }
            }
        }
        out
    }
}

/// Pack Grains: grains with sizes, aspect ratios and shapes drawn from the goal statistics,
/// moved about until they overlap and leave gaps as little as possible, then every cell
/// given to the grain whose shape fits it best (the space no grain reaches is a matrix phase
/// when the goal fraction is below 1).
pub fn pack_grains(p: &PackParams, pick_orientation: Option<&dyn Fn(&mut dyn Draws) -> Quat>) -> Result<DataSet> {
    let mut out = pack_once(p, pick_orientation)?;
    if !p.calibrate || p.fraction < 0.999 {
        return Ok(out);
    }
    // Space-filling grains measure more irregular (lower aspect ratios) and a little smaller
    // than the shapes they were drawn as: shift the goals by what is missing and pack again
    // (same seed), up to four times.
    let mean = |ab: (f64, f64)| ab.0 / (ab.0 + ab.1);
    let with_mean = |ab: (f64, f64), m: f64| {
        let k = ab.0 + ab.1;
        let m = m.clamp(0.05, 0.98);
        (m * k, (1.0 - m) * k)
    };
    let (goal_mu, goal_ba, goal_ca) = (p.esd_mu, mean(p.ba), mean(p.ca));
    let mut q = p.clone();
    for _ in 0..4 {
        let st = ensemble_statistics(&out, "FeatureIds", false).or_else(|_| ensemble_statistics(&out, "FeatureIds", true))?;
        let g = |k: &str| st.field_data().get(k).map(|a| a.get(0, 0)).unwrap_or(f64::NAN);
        let (mu, ba, ca) = (g("ESD mu"), g("b/a mean"), g("c/a mean"));
        if !(mu.is_finite() && ba.is_finite() && ca.is_finite()) {
            break;
        }
        if (mu - goal_mu).abs() < 0.02 && (ba - goal_ba).abs() < 0.02 && (ca - goal_ca).abs() < 0.02 {
            break;
        }
        q.esd_mu += goal_mu - mu;
        q.ba = with_mean(q.ba, mean(q.ba) * goal_ba / ba);
        q.ca = with_mean(q.ca, mean(q.ca) * goal_ca / ca);
        out = pack_once(&q, pick_orientation)?;
    }
    Ok(out)
}

fn pack_once(p: &PackParams, pick_orientation: Option<&dyn Fn(&mut dyn Draws) -> Quat>) -> Result<DataSet> {
    let n = p.cells.map(|c| c.max(1));
    if n[0] * n[1] * n[2] > 50_000_000 {
        bail!("more than 50 million cells");
    }
    let laue = Laue::from_label(&p.structure).ok_or_else(|| anyhow!("unknown crystal structure {}", p.structure))?;
    let h = p.spacing;
    let size: [f64; 3] = std::array::from_fn(|a| n[a] as f64 * h);
    let shape = GrainShape::of(&p.shape, p.exponent);
    let mut r = Rng::new(p.seed);
    let box_vol = size[0] * size[1] * size[2];
    let mut grains = draw_grains(p, shape, size, p.fraction.clamp(0.01, 1.0) * box_vol, &mut r)?;
    let lat = Lattice { n, h, periodic: p.periodic };
    let nc = n[0] * n[1] * n[2];
    let fill = p.fraction >= 0.999;
    // Placement: cell ownership counts; the error counts overlaps (and gaps when the grains
    // fill the box). A grain moves (a jump anywhere or a nudge) when the error drops.
    let mut own = vec![0u16; nc];
    let mut cells: Vec<Vec<usize>> = grains.iter().map(|g| lat.cells_of(g, shape)).collect();
    for cs in &cells {
        for c in cs {
            own[*c] = own[*c].saturating_add(1);
        }
    }
    let cost = |o: u16| -> i64 {
        match o {
            0 => fill as i64,
            1 => 0,
            k => (k as i64 - 1) * (k as i64 - 1),
        }
    };
    // Below a full volume, grains are added until they cover the goal fraction (overlaps
    // count once).
    for round in 0..8 {
        if round > 0 {
            let covered = own.iter().filter(|o| **o > 0).count() as f64 / nc as f64;
            if fill || covered >= p.fraction - 0.005 {
                break;
            }
            let extra = draw_grains(p, shape, size, (p.fraction - covered) * box_vol, &mut r)?;
            for g in extra {
                let cs = lat.cells_of(&g, shape);
                for c in &cs {
                    own[*c] = own[*c].saturating_add(1);
                }
                cells.push(cs);
                grains.push(g);
            }
        }
        let moves = p.moves.max(1) * grains.len();
        for _ in 0..moves {
            let g = (r.random() * grains.len() as f64) as usize % grains.len();
            let old_c = grains[g].c;
            let jump = r.random() < 0.5;
            let reach = grains[g].reach();
            let new_c: [f64; 3] = std::array::from_fn(|k| {
                let x = if jump { r.uniform(0.0, size[k]) } else { old_c[k] + r.uniform(-0.5, 0.5) * reach };
                if p.periodic {
                    x.rem_euclid(size[k])
                } else {
                    x.clamp(0.0, size[k])
                }
            });
            grains[g].c = new_c;
            let new_cells = lat.cells_of(&grains[g], shape);
            let mut delta = 0i64;
            for c in &cells[g] {
                delta += cost(own[*c] - 1) - cost(own[*c]);
                own[*c] -= 1;
            }
            for c in &new_cells {
                delta += cost(own[*c] + 1) - cost(own[*c]);
                own[*c] += 1;
            }
            if delta <= 0 {
                cells[g] = new_cells;
            } else {
                for c in &new_cells {
                    own[*c] -= 1;
                }
                for c in &cells[g] {
                    own[*c] += 1;
                }
                grains[g].c = old_c;
            }
        }
        if fill {
            break;
        }
    }
    // Assignment: each cell to the grain of smallest shape norm among those near it.
    let reach = grains.iter().map(|g| g.reach()).fold(0.0, f64::max);
    let nb: [usize; 3] = std::array::from_fn(|a| ((size[a] / reach).floor() as usize).clamp(1, 64));
    let bw: [f64; 3] = std::array::from_fn(|a| size[a] / nb[a] as f64);
    let mut bucket: Vec<Vec<u32>> = vec![Vec::new(); nb[0] * nb[1] * nb[2]];
    for (i, g) in grains.iter().enumerate() {
        let b: [usize; 3] = std::array::from_fn(|a| ((g.c[a] / bw[a]) as usize).min(nb[a] - 1));
        bucket[b[0] + nb[0] * (b[1] + nb[1] * b[2])].push(i as u32);
    }
    let labels: Vec<u32> = (0..nc)
        .into_par_iter()
        .map(|c| {
            let q = [c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1])];
            let x: [f64; 3] = std::array::from_fn(|a| (q[a] as f64 + 0.5) * h);
            let pb: [i64; 3] = std::array::from_fn(|a| ((x[a] / bw[a]) as i64).min(nb[a] as i64 - 1));
            let mut best = (f64::INFINITY, u32::MAX);
            for ring in 1..=nb.iter().copied().max().unwrap() as i64 {
                for dz in -ring..=ring {
                    for dy in -ring..=ring {
                        for dx in -ring..=ring {
                            let b = [pb[0] + dx, pb[1] + dy, pb[2] + dz];
                            let mut idx = [0usize; 3];
                            let mut ok = true;
                            for a in 0..3 {
                                if b[a] < 0 || b[a] >= nb[a] as i64 {
                                    if p.periodic {
                                        idx[a] = b[a].rem_euclid(nb[a] as i64) as usize;
                                    } else {
                                        ok = false;
                                    }
                                } else {
                                    idx[a] = b[a] as usize;
                                }
                            }
                            if !ok {
                                continue;
                            }
                            for &gi in &bucket[idx[0] + nb[0] * (idx[1] + nb[1] * idx[2])] {
                                let g = &grains[gi as usize];
                                let d: [f64; 3] = std::array::from_fn(|a| {
                                    let mut d = x[a] - g.c[a];
                                    if p.periodic {
                                        d -= size[a] * (d / size[a]).round();
                                    }
                                    d
                                });
                                let u: [f64; 3] = std::array::from_fn(|a| (g.rot[a][0] * d[0] + g.rot[a][1] * d[1] + g.rot[a][2] * d[2]) / g.ax[a]);
                                let v = shape.norm(u);
                                // Inside: rank by the depth below the surface along the ray from
                                // the centre (|d| (1/norm - 1)), so a cell in two grains goes to
                                // the one it is deeper in; outside: by the norm.
                                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                                let key = if v <= 1.0 {
                                    if v > 0.0 {
                                        -len * (1.0 / v - 1.0)
                                    } else {
                                        f64::NEG_INFINITY
                                    }
                                } else {
                                    v
                                };
                                if key < best.0 {
                                    best = (key, gi);
                                }
                            }
                        }
                    }
                }
                if best.1 != u32::MAX {
                    break;
                }
            }
            if best.0 > 1.0 {
                u32::MAX
            } else {
                best.1
            }
            // (keys at or below 1 are inside a grain: depths are negative, norms of 1 exactly
            // are on a surface)
        })
        .collect();
    // Gaps between the grains: filled by growing the grains a layer at a time (each gap cell
    // takes the most common grain among its labelled face neighbours); with a goal fraction
    // below 1 they stay as the matrix.
    let mut labels = labels;
    if fill {
        let vox = crate::features::Vox { n, sp: [h; 3], corner: [0.0; 3], flat: [false; 3] };
        loop {
            let gaps: Vec<usize> = (0..nc).filter(|c| labels[*c] == u32::MAX).collect();
            if gaps.is_empty() {
                break;
            }
            let fills: Vec<(usize, u32)> = gaps
                .par_iter()
                .filter_map(|c| {
                    let mut nb: Vec<u32> = periodic_nbrs(&vox, *c, p.periodic).into_iter().map(|d| labels[d]).filter(|l| *l != u32::MAX).collect();
                    if nb.is_empty() {
                        return None;
                    }
                    nb.sort_unstable();
                    let mut best = (0, nb[0]);
                    let mut k = 0;
                    while k < nb.len() {
                        let j = nb[k..].iter().take_while(|x| **x == nb[k]).count();
                        if j > best.0 {
                            best = (j, nb[k]);
                        }
                        k += j;
                    }
                    Some((*c, best.1))
                })
                .collect();
            if fills.is_empty() {
                break;
            }
            for (c, l) in fills {
                labels[c] = l;
            }
        }
    }
    let (mut ids, order) = compact(&labels.iter().map(|l| if *l == u32::MAX { u32::MAX } else { *l }).collect::<Vec<_>>());
    // The matrix (cells no grain reaches) is the last feature, of phase 2.
    let matrix = order.iter().position(|l| *l == u32::MAX).map(|k| k as i32 + 1);
    let ng = order.len();
    let mut orients: Vec<Quat> = match pick_orientation {
        Some(f) => (0..ng).map(|_| f(&mut r)).collect(),
        None => grain_orientations(ng, &p.orientations, p.spread_deg.to_radians(), &mut r),
    };
    let mut phases = vec![1i32; nc];
    if let Some(m) = matrix {
        // Renumber so the matrix is the last feature.
        let last = ng as i32;
        for (c, id) in ids.iter_mut().enumerate() {
            if *id == m {
                *id = last;
                phases[c] = 2;
            } else if *id > m {
                *id -= 1;
            }
        }
        let q = orients.remove((m - 1) as usize);
        orients.push(q);
    }
    let mut img = microstructure(n, [h; 3], ids, &orients, laue);
    img.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(phases)));
    if matrix.is_some() {
        img.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(vec![Laue::UNKNOWN_CODE, laue.code(), laue.code()])));
    }
    let over = own.iter().filter(|o| **o > 1).count();
    img.field_data.insert(Array::scalars_f64("OverlapFraction", vec![over as f64 / nc as f64]));
    Ok(DataSet::ImageData(img))
}

/// The first point's orientation of every feature, and the feature's cell count.
fn feature_orientations(ds: &DataSet, ids_name: &str) -> Result<(Vec<Option<Quat>>, Vec<usize>, Laue)> {
    let (assoc, a) = crate::crystal::orientation_array(ds, None)?;
    let repr = crate::crystal::Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
    let n = a.len();
    let ids = feature_ids(ds, ids_name, assoc, n)?;
    let cls = crate::crystal::classes_of(ds, assoc, n, crate::crystal::FROM_PHASES, None).unwrap_or_else(|_| vec![Some(Laue::CubicHigh); n]);
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    let mut q = vec![None; nf];
    let mut count = vec![0usize; nf];
    let mut laue = None;
    for i in 0..n {
        if ids[i] > 0 {
            let f = ids[i] as usize;
            count[f] += 1;
            if q[f].is_none() {
                if let Some(l) = cls[i] {
                    q[f] = Some(repr.decode_values(&a, i));
                    laue.get_or_insert(l);
                }
            }
        }
    }
    Ok((q, count, laue.unwrap_or(Laue::CubicHigh)))
}

/// Pack Like: a new volume packed with the statistics measured on `ds` (its size, shape and
/// neighbour fits), the grains taking orientations of the measured grains (by volume).
pub fn pack_like(ds: &DataSet, cells: [usize; 3], spacing: f64, ids_name: &str, seed: u64) -> Result<DataSet> {
    let img = image(ds)?;
    let st = ensemble_statistics(ds, ids_name, false).or_else(|_| ensemble_statistics(ds, ids_name, true))?;
    let fd = st.field_data();
    let g = |k: &str| fd.get(k).map(|a| a.get(0, 0)).unwrap_or(f64::NAN);
    let ok = |v: f64, d: f64| if v.is_finite() && v > 0.0 { v } else { d };
    let (q, count, laue) = feature_orientations(ds, ids_name)?;
    let pool: Vec<(Quat, usize)> = q.iter().zip(&count).filter_map(|(q, c)| q.map(|q| (q, *c))).collect();
    if pool.is_empty() {
        bail!("the input has no indexed features to take orientations from");
    }
    let total: usize = pool.iter().map(|x| x.1).sum();
    let pick = move |r: &mut dyn Draws| -> Quat {
        let mut t = r.random() * total as f64;
        for (q, c) in &pool {
            t -= *c as f64;
            if t <= 0.0 {
                return *q;
            }
        }
        pool.last().unwrap().0
    };
    let sp = if spacing > 0.0 { spacing } else { img.spacing[0].abs() };
    let p = PackParams {
        cells,
        spacing: sp,
        esd_mu: g("ESD mu"),
        esd_sigma: ok(g("ESD sigma"), 0.1),
        min_cut: 3.0,
        max_cut: 3.0,
        ba: (ok(g("b/a alpha"), 15.0), ok(g("b/a beta"), 2.0)),
        ca: (ok(g("c/a alpha"), 12.0), ok(g("c/a beta"), 4.0)),
        shape: SHAPE_ELLIPSOID.into(),
        exponent: 2.0,
        aligned: false,
        fraction: 1.0,
        periodic: false,
        orientations: ORIENT_RANDOM.into(),
        spread_deg: 0.0,
        structure: laue.label().into(),
        seed,
        moves: 20,
        calibrate: true,
    };
    if !p.esd_mu.is_finite() {
        bail!("the size of the input's features could not be measured");
    }
    pack_grains(&p, Some(&pick))
}

// ------------------------------------------------------------------ precipitates

pub struct PrecipitateParams {
    pub fraction: f64,
    pub esd_mu: f64,
    pub esd_sigma: f64,
    pub at_boundaries: bool,
    pub structure: String,
    pub seed: u64,
}

/// Insert Precipitates: spherical particles of a new phase, sizes lognormal, placed at random
/// (or on grain boundaries) without overlapping one another, until they fill the goal volume
/// fraction. Each particle is a feature of its own with a random orientation.
pub fn insert_precipitates(ds: &DataSet, p: &PrecipitateParams, ids_name: &str) -> Result<DataSet> {
    let img = image(ds)?;
    let v = crate::features::Vox::of(img);
    let laue = Laue::from_label(&p.structure).ok_or_else(|| anyhow!("unknown crystal structure {}", p.structure))?;
    let ids_arr = img.cell_data.get(ids_name).ok_or_else(|| anyhow!("no cell array {ids_name}"))?;
    let nc = v.len();
    let mut ids: Vec<i32> = (0..nc).map(|i| ids_arr.get(i, 0) as i32).collect();
    let mut phases: Vec<i32> = match img.cell_data.get("Phases") {
        Some(a) => (0..nc).map(|i| a.get(i, 0) as i32).collect(),
        None => vec![1; nc],
    };
    let new_phase = phases.iter().copied().max().unwrap_or(0).max(0) + 1;
    let (name, mut q): (String, Vec<Quat>) = {
        let (_, a) = crate::crystal::orientation_array(ds, None)?;
        let repr = crate::crystal::Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
        (a.name.clone(), (0..nc).map(|i| repr.decode_values(a, i)).collect())
    };
    let mut r = Rng::new(p.seed);
    let goal = (p.fraction.clamp(0.0, 0.9) * nc as f64).round() as usize;
    let mut taken = vec![false; nc];
    let boundary: Vec<usize> = if p.at_boundaries { (0..nc).filter(|c| v.nbrs(*c).iter().flatten().any(|d| ids[*d] != ids[*c])).collect() } else { Vec::new() };
    let mut next_id = ids.iter().copied().max().unwrap_or(0) + 1;
    let mut placed = 0usize;
    let mut misses = 0;
    while placed < goal && misses < 2000 {
        let esd = (p.esd_mu + p.esd_sigma * r.std_normal().clamp(-3.0, 3.0)).exp();
        let rad = 0.5 * esd;
        let centre: [f64; 3] = if p.at_boundaries && !boundary.is_empty() {
            v.center(boundary[(r.random() * boundary.len() as f64) as usize % boundary.len()])
        } else {
            std::array::from_fn(|a| v.corner[a] + r.uniform(0.0, v.n[a] as f64 * v.sp[a]))
        };
        let lo: [i64; 3] = std::array::from_fn(|a| ((centre[a] - rad - v.corner[a]) / v.sp[a]).floor() as i64);
        let hi: [i64; 3] = std::array::from_fn(|a| ((centre[a] + rad - v.corner[a]) / v.sp[a]).ceil() as i64);
        let mut cells = Vec::new();
        for k in lo[2].max(0)..=hi[2].min(v.n[2] as i64 - 1) {
            for j in lo[1].max(0)..=hi[1].min(v.n[1] as i64 - 1) {
                for i in lo[0].max(0)..=hi[0].min(v.n[0] as i64 - 1) {
                    let c = i as usize + v.n[0] * (j as usize + v.n[1] * k as usize);
                    let x = v.center(c);
                    if (0..3).map(|a| (x[a] - centre[a]).powi(2)).sum::<f64>() <= rad * rad {
                        cells.push(c);
                    }
                }
            }
        }
        // Particles keep apart: no cell of another particle within the particle or next to it.
        if cells.is_empty() || cells.iter().any(|c| taken[*c] || v.nbrs(*c).iter().flatten().any(|d| taken[*d])) {
            misses += 1;
            continue;
        }
        let qn = random_quat(&mut r);
        for c in &cells {
            taken[*c] = true;
            ids[*c] = next_id;
            phases[*c] = new_phase;
            q[*c] = qn;
        }
        next_id += 1;
        placed += cells.len();
    }
    let mut out = img.clone();
    out.cell_data.insert(Array::new(ids_name, 1, ArrayData::I32(ids)));
    out.cell_data.insert(Array::new("Phases", 1, ArrayData::I32(phases)));
    let (_, a) = crate::crystal::orientation_array(ds, None)?;
    let mut arr = encode_like(a, &q)?;
    arr.name = name;
    out.cell_data.insert(arr);
    let mut cs: Vec<u32> = match img.field_data.get("CrystalStructures") {
        Some(a) => (0..a.len()).map(|i| a.get(i, 0) as u32).collect(),
        None => vec![Laue::UNKNOWN_CODE, Laue::CubicHigh.code()],
    };
    cs.resize(new_phase as usize, Laue::UNKNOWN_CODE);
    cs.push(laue.code());
    out.field_data.insert(Array::new("CrystalStructures", 1, ArrayData::U32(cs)));
    out.field_data.insert(Array::scalars_f64("PrecipitateFraction", vec![placed as f64 / nc as f64]));
    Ok(DataSet::ImageData(out))
}

// ------------------------------------------------------------------ crystallography

/// Match Crystallography: the grains of `ds` (phase 1) take orientations drawn from the
/// grains of `reference` (by volume, so the texture matches), then swap orientations among
/// themselves (or draw new ones) while that brings the distribution of disorientations
/// across their boundaries (weighted by boundary area, 5-degree bins) closer to the
/// reference's.
pub fn match_crystallography(ds: &DataSet, reference: &DataSet, ids_name: &str, moves: usize, seed: u64) -> Result<DataSet> {
    let img = image(ds)?;
    let v = crate::features::Vox::of(img);
    let (rq, rcount, laue) = feature_orientations(reference, ids_name)?;
    let pool: Vec<(Quat, usize)> = rq.iter().zip(&rcount).filter_map(|(q, c)| q.map(|q| (q, *c))).collect();
    if pool.is_empty() {
        bail!("the reference has no indexed features");
    }
    let bins = 13usize; // 5-degree bins up to 65 degrees
    let bin_of = |a: f64| ((a.to_degrees() / 5.0) as usize).min(bins - 1);
    // Boundary areas between feature pairs (face counts).
    let pairs_of = |d: &DataSet| -> Result<std::collections::HashMap<(usize, usize), f64>> {
        let im = image(d)?;
        let vx = crate::features::Vox::of(im);
        let a = im.cell_data.get(ids_name).ok_or_else(|| anyhow!("no cell array {ids_name}"))?;
        let ids: Vec<i64> = (0..vx.len()).map(|i| a.get(i, 0) as i64).collect();
        let mut m = std::collections::HashMap::new();
        for c in 0..vx.len() {
            for d in vx.nbrs(c)[3..].iter().flatten() {
                let (x, y) = (ids[c], ids[*d]);
                if x > 0 && y > 0 && x != y {
                    *m.entry((x.min(y) as usize, x.max(y) as usize)).or_insert(0.0) += 1.0;
                }
            }
        }
        Ok(m)
    };
    let hist = |pairs: &std::collections::HashMap<(usize, usize), f64>, q: &[Option<Quat>]| -> Vec<f64> {
        let mut h = vec![0.0; bins];
        for ((a, b), w) in pairs {
            if let (Some(x), Some(y)) = (q.get(*a).copied().flatten(), q.get(*b).copied().flatten()) {
                h[bin_of(cx::disorientation_angle(x, y, laue))] += w;
            }
        }
        let s: f64 = h.iter().sum::<f64>().max(1e-300);
        h.iter().map(|x| x / s).collect()
    };
    let rpairs = pairs_of(reference)?;
    let goal = hist(&rpairs, &rq);
    let pairs = pairs_of(ds)?;
    let ida = img.cell_data.get(ids_name).ok_or_else(|| anyhow!("no cell array {ids_name}"))?;
    let ids: Vec<i64> = (0..v.len()).map(|i| ida.get(i, 0) as i64).collect();
    let phases: Vec<i32> = match img.cell_data.get("Phases") {
        Some(a) => (0..v.len()).map(|i| a.get(i, 0) as i32).collect(),
        None => vec![1; v.len()],
    };
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    let mut phase_of = vec![0i32; nf];
    for c in 0..v.len() {
        if ids[c] > 0 {
            phase_of[ids[c] as usize] = phases[c];
        }
    }
    let mine: Vec<usize> = (1..nf).filter(|f| phase_of[*f] == 1).collect();
    let mut r = Rng::new(seed);
    let total: usize = pool.iter().map(|x| x.1).sum();
    let draw = |r: &mut Rng| -> Quat {
        let mut t = r.random() * total as f64;
        for (q, c) in &pool {
            t -= *c as f64;
            if t <= 0.0 {
                return *q;
            }
        }
        pool.last().unwrap().0
    };
    let mut q: Vec<Option<Quat>> = vec![None; nf];
    for f in &mine {
        q[*f] = Some(draw(&mut r));
    }
    // Incremental histogram: the pairs of each feature.
    let mut by_feature: Vec<Vec<(usize, f64)>> = vec![Vec::new(); nf];
    for ((a, b), w) in &pairs {
        by_feature[*a].push((*b, *w));
        by_feature[*b].push((*a, *w));
    }
    let wsum: f64 = pairs.values().sum::<f64>().max(1e-300);
    let mut h: Vec<f64> = vec![0.0; bins];
    for ((a, b), w) in &pairs {
        if let (Some(x), Some(y)) = (q[*a], q[*b]) {
            h[bin_of(cx::disorientation_angle(x, y, laue))] += w / wsum;
        }
    }
    let err = |h: &[f64]| -> f64 { h.iter().zip(&goal).map(|(a, b)| (a - b).abs()).sum() };
    let before = err(&h);
    let contrib = |f: usize, qf: Option<Quat>, q: &[Option<Quat>], skip: usize, h: &mut [f64], sign: f64| {
        for (g, w) in &by_feature[f] {
            if *g == skip {
                continue;
            }
            if let (Some(x), Some(y)) = (qf, q[*g]) {
                h[bin_of(cx::disorientation_angle(x, y, laue))] += sign * w / wsum;
            }
        }
    };
    let mut current = before;
    if mine.len() >= 2 {
        for _ in 0..moves.max(1) * mine.len() {
            let a = mine[(r.random() * mine.len() as f64) as usize % mine.len()];
            let swap = r.random() < 0.7;
            let b = mine[(r.random() * mine.len() as f64) as usize % mine.len()];
            if swap && a == b {
                continue;
            }
            let mut trial = h.clone();
            let (qa, qb) = (q[a], q[b]);
            let (na, nb) = if swap { (qb, qa) } else { (Some(draw(&mut r)), qb) };
            // Remove the old pairs of a (and b), add the new ones.
            contrib(a, qa, &q, usize::MAX, &mut trial, -1.0);
            if swap {
                contrib(b, qb, &q, a, &mut trial, -1.0);
            }
            let mut q2 = q.clone();
            q2[a] = na;
            if swap {
                q2[b] = nb;
            }
            contrib(a, na, &q2, usize::MAX, &mut trial, 1.0);
            if swap {
                contrib(b, nb, &q2, a, &mut trial, 1.0);
            }
            let e = err(&trial);
            if e < current {
                current = e;
                h = trial;
                q = q2;
            }
        }
    }
    let (assoc, a) = crate::crystal::orientation_array(ds, None)?;
    let repr = crate::crystal::Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
    let qs: Vec<Quat> =
        (0..a.len()).map(|i| if ids[i] > 0 { q[ids[i] as usize].unwrap_or_else(|| repr.decode_values(a, i)) } else { repr.decode_values(a, i) }).collect();
    let mut out = replace(ds, assoc, encode_like(a, &qs)?)?;
    let fd = out.field_data_mut_any();
    fd.insert(Array::scalars_f64("MDFErrorBefore", vec![before]));
    fd.insert(Array::scalars_f64("MDFErrorAfter", vec![current]));
    Ok(out)
}

/// Face neighbours of a cell, wrapping across the faces when periodic.
fn periodic_nbrs(v: &crate::features::Vox, c: usize, periodic: bool) -> Vec<usize> {
    if !periodic {
        return v.nbrs(c).into_iter().flatten().collect();
    }
    let q = v.ijk(c);
    let mut out = Vec::with_capacity(6);
    for a in 0..3 {
        for d in [-1i64, 1] {
            let mut r = q.map(|x| x as i64);
            r[a] = (r[a] + d).rem_euclid(v.n[a] as i64);
            out.push(r[0] as usize + v.n[0] * (r[1] as usize + v.n[1] * r[2] as usize));
        }
    }
    out
}
