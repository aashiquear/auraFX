//! Properties of a microstructure computed on its grid of cells, the box taken as one period
//! of an endless medium: the apparent conductivity and stiffness (a mean gradient or strain is
//! applied and the local fields found by conjugate gradients, the fields kept compatible in
//! Fourier space with the differences of the corners of each cell), the bounds that the
//! volume fractions alone give, unit cells of best stiffness or conductivity found by moving
//! material, and the frequencies of elastic waves through the periodic medium.

use super::bio::phases_of;
use super::fft::{symmetric_eigenvalues, Fft3, C};
use super::*;
use crate::synthetic::Rng;
use anyhow::{bail, Result};
use std::f64::consts::PI;

/// The grid in Fourier space: for every frequency the unit direction along which a gradient
/// of that frequency points (zero for the mean).
pub struct Spectral {
    fft: Fft3,
    dirs: Vec<[f64; 3]>,
}

impl Spectral {
    pub fn new(n: [usize; 3]) -> Spectral {
        let fft = Fft3::new(n);
        let dirs = (0..fft.len())
            .into_par_iter()
            .map(|c| {
                let m = fft.index(c);
                // Differences between the corners of a cell: sin along the axis, cos across.
                let half: [f64; 3] = std::array::from_fn(|k| PI * m[k] as f64 / n[k] as f64);
                let t: [f64; 3] = std::array::from_fn(|k| half[k].sin() * half[(k + 1) % 3].cos() * half[(k + 2) % 3].cos());
                let l = norm(t);
                if l > 1e-12 {
                    scale(t, 1.0 / l)
                } else {
                    [0.0; 3]
                }
            })
            .collect();
        Spectral { fft, dirs }
    }

    /// Keep of a field its compatible part of zero mean.
    fn project<const M: usize>(&self, v: &mut [[f64; M]], rule: &(dyn Fn(&[f64; 3], &mut [C; M]) + Sync)) {
        let mut comps: Vec<Vec<C>> = (0..M).map(|j| v.par_iter().map(|x| [x[j], 0.0]).collect()).collect();
        comps.iter_mut().for_each(|c| self.fft.run(c, false));
        let out: Vec<[C; M]> = (0..v.len())
            .into_par_iter()
            .map(|c| {
                let mut t: [C; M] = std::array::from_fn(|j| comps[j][c]);
                if self.dirs[c] == [0.0; 3] {
                    t = [[0.0; 2]; M];
                } else {
                    rule(&self.dirs[c], &mut t);
                }
                t
            })
            .collect();
        for (j, comp) in comps.iter_mut().enumerate() {
            comp.par_iter_mut().zip(&out).for_each(|(a, t)| *a = t[j]);
            self.fft.run(comp, true);
        }
        v.par_iter_mut().enumerate().for_each(|(c, x)| {
            for j in 0..M {
                x[j] = comps[j][c][0];
            }
        });
    }
}

pub struct Solved<const M: usize> {
    /// The local gradient (or strain), its mean the one applied.
    pub field: Vec<[f64; M]>,
    /// The mean of the flux (or stress).
    pub mean_flux: [f64; M],
    pub iterations: usize,
    pub residual: f64,
}

/// The field of mean `load` whose flux `law(cell, field)` is in balance, by conjugate
/// gradients on its fluctuation (`guess`: a fluctuation to start from).
#[allow(clippy::too_many_arguments)]
fn solve<const M: usize>(
    sp: &Spectral,
    law: &(dyn Fn(usize, &[f64; M]) -> [f64; M] + Sync),
    rule: &(dyn Fn(&[f64; 3], &mut [C; M]) + Sync),
    weights: [f64; M],
    load: [f64; M],
    tolerance: f64,
    max_iter: usize,
    guess: Option<&[[f64; M]]>,
) -> Solved<M> {
    let n = sp.fft.len();
    let inner = |a: &[[f64; M]], b: &[[f64; M]]| -> f64 { a.par_iter().zip(b).map(|(x, y)| (0..M).map(|j| weights[j] * x[j] * y[j]).sum::<f64>()).sum() };
    let apply = |x: &[[f64; M]], shift: [f64; M]| -> Vec<[f64; M]> {
        x.par_iter().enumerate().map(|(c, e)| law(c, &std::array::from_fn(|j| e[j] + shift[j]))).collect()
    };
    let mut x: Vec<[f64; M]> = guess.map(|g| g.to_vec()).unwrap_or_else(|| vec![[0.0; M]; n]);
    let mut r = apply(&x, load);
    let reference = inner(&r, &r).sqrt().max(1e-300);
    sp.project(&mut r, rule);
    r.par_iter_mut().for_each(|v| v.iter_mut().for_each(|a| *a = -*a));
    let mut rr = inner(&r, &r);
    let mut p = r.clone();
    let mut iterations = 0;
    while iterations < max_iter && rr.sqrt() > tolerance * reference {
        let mut ap = apply(&p, [0.0; M]);
        let pcp = inner(&p, &ap);
        if !(pcp > 0.0) {
            break;
        }
        sp.project(&mut ap, rule);
        let alpha = rr / pcp;
        x.par_iter_mut().zip(&p).for_each(|(a, b)| (0..M).for_each(|j| a[j] += alpha * b[j]));
        r.par_iter_mut().zip(&ap).for_each(|(a, b)| (0..M).for_each(|j| a[j] -= alpha * b[j]));
        let next = inner(&r, &r);
        let beta = next / rr;
        rr = next;
        p.par_iter_mut().zip(&r).for_each(|(a, b)| (0..M).for_each(|j| a[j] = b[j] + beta * a[j]));
        iterations += 1;
    }
    x.par_iter_mut().for_each(|a| (0..M).for_each(|j| a[j] += load[j]));
    let flux = apply(&x, [0.0; M]);
    let mean_flux: [f64; M] = std::array::from_fn(|j| flux.par_iter().map(|f| f[j]).sum::<f64>() / n as f64);
    Solved { field: x, mean_flux, iterations, residual: rr.sqrt() / reference }
}

/// A gradient: the part along the direction.
fn gradient_rule(d: &[f64; 3], t: &mut [C; 3]) {
    for part in 0..2 {
        let s = d[0] * t[0][part] + d[1] * t[1][part] + d[2] * t[2][part];
        for k in 0..3 {
            t[k][part] = d[k] * s;
        }
    }
}

/// A strain (xx, yy, zz, yz, xz, xy): the symmetric gradient of a displacement.
fn strain_rule(d: &[f64; 3], t: &mut [C; 6]) {
    for part in 0..2 {
        let s = [t[0][part], t[1][part], t[2][part], t[3][part], t[4][part], t[5][part]];
        let u = [s[0] * d[0] + s[5] * d[1] + s[4] * d[2], s[5] * d[0] + s[1] * d[1] + s[3] * d[2], s[4] * d[0] + s[3] * d[1] + s[2] * d[2]];
        let du = dot(*d, u);
        let e = |i: usize, j: usize| d[i] * u[j] + d[j] * u[i] - d[i] * d[j] * du;
        let out = [e(0, 0), e(1, 1), e(2, 2), e(1, 2), e(0, 2), e(0, 1)];
        for k in 0..6 {
            t[k][part] = out[k];
        }
    }
}

const STRAIN_WEIGHTS: [f64; 6] = [1.0, 1.0, 1.0, 2.0, 2.0, 2.0];

fn stress(lam: f64, mu: f64, e: &[f64; 6]) -> [f64; 6] {
    let tr = lam * (e[0] + e[1] + e[2]);
    [tr + 2.0 * mu * e[0], tr + 2.0 * mu * e[1], tr + 2.0 * mu * e[2], 2.0 * mu * e[3], 2.0 * mu * e[4], 2.0 * mu * e[5]]
}

/// The value of a list for a phase: the entry of that phase, the last one beyond the list.
pub fn of_phase(values: &[f64], phase: i32) -> f64 {
    values.get((phase.max(1) - 1) as usize).or(values.last()).copied().unwrap_or(1.0)
}

fn fractions(phases: &[i32]) -> Vec<(i32, f64)> {
    let mut count = std::collections::BTreeMap::new();
    phases.iter().for_each(|p| *count.entry(*p).or_insert(0usize) += 1);
    count.into_iter().map(|(p, c)| (p, c as f64 / phases.len() as f64)).collect()
}

/// The bounds of Hashin and Shtrikman on a conductivity in `dim` dimensions: (lower, upper).
pub fn conductivity_bounds(parts: &[(f64, f64)], dim: f64) -> (f64, f64) {
    let side = |k0: f64| -> f64 {
        let s: f64 = parts.iter().map(|(f, k)| f / (k + (dim - 1.0) * k0)).sum();
        1.0 / s - (dim - 1.0) * k0
    };
    let lo = parts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let hi = parts.iter().map(|p| p.1).fold(0.0, f64::max);
    (if lo > 0.0 { side(lo) } else { 0.0 }, side(hi))
}

/// The bounds of Hashin and Shtrikman on the bulk and shear moduli of an isotropic mixture
/// of isotropic phases (fraction, bulk, shear): ((lower, upper) bulk, (lower, upper) shear).
pub fn elastic_bounds(parts: &[(f64, f64, f64)]) -> ((f64, f64), (f64, f64)) {
    let bulk = |g0: f64| 1.0 / parts.iter().map(|(f, k, _)| f / (k + 4.0 / 3.0 * g0)).sum::<f64>() - 4.0 / 3.0 * g0;
    let shear = |k0: f64, g0: f64| {
        let z = if k0 + 2.0 * g0 > 0.0 { g0 / 6.0 * (9.0 * k0 + 8.0 * g0) / (k0 + 2.0 * g0) } else { 0.0 };
        let s: f64 = parts.iter().map(|(f, _, g)| f / (g + z)).sum();
        if s.is_finite() {
            1.0 / s - z
        } else {
            0.0
        }
    };
    let (kmin, kmax) = (parts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min), parts.iter().map(|p| p.1).fold(0.0, f64::max));
    let (gmin, gmax) = (parts.iter().map(|p| p.2).fold(f64::INFINITY, f64::min), parts.iter().map(|p| p.2).fold(0.0, f64::max));
    ((bulk(gmin), bulk(gmax)), (shear(kmin, gmin), shear(kmax, gmax)))
}

fn cubic_cells(img: &ImageData) -> Result<([usize; 3], f64)> {
    let v = crate::features::Vox::of(img);
    let h = v.sp[0];
    if (0..3).any(|k| v.n[k] > 1 && (v.sp[k] - h).abs() > 1e-9 * h) {
        bail!("the cells must be cubes (the same size along every axis)");
    }
    Ok((v.n, h))
}

/// Effective Conductivity: a mean gradient along each axis in turn, the mean flux it gives.
/// The gradient and the flux of the load along `axis` are kept as cell arrays.
pub fn effective_conductivity(ds: &DataSet, array: &str, conductivities: &[f64], axis: usize, tolerance: f64, max_iter: usize) -> Result<DataSet> {
    let (mut img, phases) = phases_of(ds, array)?;
    let (n, _) = cubic_cells(&img)?;
    if conductivities.is_empty() || conductivities.iter().any(|k| !(*k >= 0.0)) {
        bail!("give a conductivity (0 or above) for every phase");
    }
    let k: Vec<f64> = phases.iter().map(|p| of_phase(conductivities, *p)).collect();
    let sp = Spectral::new(n);
    let law = |c: usize, e: &[f64; 3]| -> [f64; 3] { [k[c] * e[0], k[c] * e[1], k[c] * e[2]] };
    let mut tensor = [[0.0; 3]; 3];
    let (mut iterations, mut residual) = (0usize, 0.0f64);
    for a in 0..3 {
        let mut load = [0.0; 3];
        load[a] = 1.0;
        let s = solve(&sp, &law, &gradient_rule, [1.0; 3], load, tolerance, max_iter, None);
        for b in 0..3 {
            tensor[b][a] = s.mean_flux[b];
        }
        iterations = iterations.max(s.iterations);
        residual = residual.max(s.residual);
        if a == axis.min(2) {
            img.cell_data.insert(Array::new("Gradient", 3, ArrayData::F32(s.field.iter().flatten().map(|v| *v as f32).collect())));
            img.cell_data.insert(Array::new("Flux", 3, ArrayData::F32(s.field.iter().enumerate().flat_map(|(c, e)| law(c, e)).map(|v| v as f32).collect())));
        }
    }
    let parts: Vec<(f64, f64)> = fractions(&phases).iter().map(|(p, f)| (*f, of_phase(conductivities, *p))).collect();
    let mean: f64 = parts.iter().map(|(f, k)| f * k).sum();
    let harmonic = if parts.iter().all(|p| p.1 > 0.0) { 1.0 / parts.iter().map(|(f, k)| f / k).sum::<f64>() } else { 0.0 };
    let (lo, hi) = conductivity_bounds(&parts, if n[2] == 1 { 2.0 } else { 3.0 });
    let best = parts.iter().cloned().fold((0.0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
    note(&mut img, "EffectiveConductivity", tensor.iter().flatten().copied().collect());
    note(&mut img, "ConductivityBounds", vec![harmonic, lo, hi, mean]);
    // How much longer and narrower the paths through the best conductor are than straight ones.
    note(&mut img, "TortuosityFactor", (0..3).map(|a| best.0 * best.1 / tensor[a][a]).collect());
    note(&mut img, "SolverIterations", vec![iterations as f64]);
    note(&mut img, "SolverResidual", vec![residual]);
    Ok(DataSet::ImageData(img))
}

/// Lame's first parameter and the shear modulus from Young's modulus and Poisson's ratio.
pub fn lame(young: f64, poisson: f64) -> (f64, f64) {
    (young * poisson / ((1.0 + poisson) * (1.0 - 2.0 * poisson)), young / (2.0 * (1.0 + poisson)))
}

/// The inverse of a 6 x 6 matrix (none when it is singular).
fn invert6(m: [[f64; 6]; 6]) -> Option<[[f64; 6]; 6]> {
    let mut a = [[0.0; 12]; 6];
    for i in 0..6 {
        a[i][..6].copy_from_slice(&m[i]);
        a[i][6 + i] = 1.0;
    }
    for c in 0..6 {
        let p = (c..6).max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs()))?;
        if a[p][c].abs() < 1e-300 {
            return None;
        }
        a.swap(c, p);
        let d = a[c][c];
        a[c].iter_mut().for_each(|v| *v /= d);
        for r in 0..6 {
            if r != c {
                let f = a[r][c];
                for k in 0..12 {
                    a[r][k] -= f * a[c][k];
                }
            }
        }
    }
    Some(std::array::from_fn(|i| std::array::from_fn(|j| a[i][6 + j])))
}

pub const LOADS: &[&str] = &["Stretch along X", "Stretch along Y", "Stretch along Z", "Shear YZ", "Shear XZ", "Shear XY"];

fn unit_strain(case: usize) -> [f64; 6] {
    let mut e = [0.0; 6];
    // A unit engineering shear is half a unit of the tensor component.
    e[case] = if case < 3 { 1.0 } else { 0.5 };
    e
}

/// The stiffness matrix (rows and columns xx, yy, zz, yz, xz, xy, engineering shears) of the
/// medium of local Lame parameters `lam` and `mu`, with the strain fields of the six loads
/// when `keep` asks for them.
fn stiffness(
    sp: &Spectral,
    lam: &[f64],
    mu: &[f64],
    tolerance: f64,
    max_iter: usize,
    keep: Option<usize>,
) -> ([[f64; 6]; 6], Option<Vec<[f64; 6]>>, usize, f64) {
    let law = |c: usize, e: &[f64; 6]| stress(lam[c], mu[c], e);
    let mut cm = [[0.0; 6]; 6];
    let (mut kept, mut iterations, mut residual) = (None, 0usize, 0.0f64);
    for case in 0..6 {
        let s = solve(sp, &law, &strain_rule, STRAIN_WEIGHTS, unit_strain(case), tolerance, max_iter, None);
        for row in 0..6 {
            cm[row][case] = s.mean_flux[row];
        }
        iterations = iterations.max(s.iterations);
        residual = residual.max(s.residual);
        if keep == Some(case) {
            kept = Some(s.field);
        }
    }
    (cm, kept, iterations, residual)
}

/// Effective Stiffness: six mean strains in turn, the mean stresses they give. The strain,
/// the stress and the von Mises stress of the load `load` are kept as cell arrays.
pub fn effective_stiffness(ds: &DataSet, array: &str, youngs: &[f64], poisson: &[f64], load: usize, tolerance: f64, max_iter: usize) -> Result<DataSet> {
    let (mut img, phases) = phases_of(ds, array)?;
    let (n, _) = cubic_cells(&img)?;
    if youngs.is_empty() || youngs.iter().any(|e| !(*e >= 0.0)) || poisson.iter().any(|v| !(*v > -1.0 && *v < 0.5)) {
        bail!("give a Young's modulus (0 or above) and a Poisson's ratio (between -1 and 0.5) for every phase");
    }
    let pairs: Vec<(f64, f64)> = phases.iter().map(|p| lame(of_phase(youngs, *p), if poisson.is_empty() { 0.3 } else { of_phase(poisson, *p) })).collect();
    let (lam, mu): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();
    let sp = Spectral::new(n);
    let load = load.min(5);
    let (cm, field, iterations, residual) = stiffness(&sp, &lam, &mu, tolerance, max_iter, Some(load));
    let field = field.unwrap();
    let sig: Vec<[f64; 6]> = field.par_iter().enumerate().map(|(c, e)| stress(lam[c], mu[c], e)).collect();
    let mises: Vec<f32> = sig
        .iter()
        .map(|s| {
            (0.5 * ((s[0] - s[1]).powi(2) + (s[1] - s[2]).powi(2) + (s[2] - s[0]).powi(2)) + 3.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5])).sqrt() as f32
        })
        .collect();
    img.cell_data.insert(Array::new("Strain", 6, ArrayData::F32(field.iter().flatten().map(|v| *v as f32).collect())));
    img.cell_data.insert(Array::new("Stress", 6, ArrayData::F32(sig.iter().flatten().map(|v| *v as f32).collect())));
    img.cell_data.insert(Array::scalars_f32("VonMisesStress", mises));
    note(&mut img, "EffectiveStiffness", cm.iter().flatten().copied().collect());
    if let Some(s) = invert6(cm) {
        note(&mut img, "EffectiveModuli", vec![1.0 / s[0][0], 1.0 / s[1][1], 1.0 / s[2][2], 1.0 / s[3][3], 1.0 / s[4][4], 1.0 / s[5][5]]);
        note(&mut img, "PoissonRatios", vec![-s[1][0] / s[0][0], -s[2][0] / s[0][0], -s[2][1] / s[1][1]]);
    }
    let bulk = (cm[0][0] + cm[1][1] + cm[2][2] + 2.0 * (cm[0][1] + cm[0][2] + cm[1][2])) / 9.0;
    let shear = (cm[0][0] + cm[1][1] + cm[2][2] - cm[0][1] - cm[0][2] - cm[1][2] + 3.0 * (cm[3][3] + cm[4][4] + cm[5][5])) / 15.0;
    note(&mut img, "BulkModulus", vec![bulk]);
    note(&mut img, "ShearModulus", vec![shear]);
    // 1 for an isotropic medium: the shear stiffness along the axes over that across them.
    let c11 = (cm[0][0] + cm[1][1] + cm[2][2]) / 3.0;
    let c12 = (cm[0][1] + cm[0][2] + cm[1][2]) / 3.0;
    let c44 = (cm[3][3] + cm[4][4] + cm[5][5]) / 3.0;
    note(&mut img, "AnisotropyRatio", vec![2.0 * c44 / (c11 - c12)]);
    let parts: Vec<(f64, f64, f64)> = fractions(&phases)
        .iter()
        .map(|(p, f)| {
            let (l, m) = lame(of_phase(youngs, *p), if poisson.is_empty() { 0.3 } else { of_phase(poisson, *p) });
            (*f, l + 2.0 / 3.0 * m, m)
        })
        .collect();
    let ((klo, khi), (glo, ghi)) = elastic_bounds(&parts);
    let voigt = |k: usize| parts.iter().map(|p| p.0 * [p.1, p.2][k]).sum::<f64>();
    let reuss = |k: usize| if parts.iter().all(|p| [p.1, p.2][k] > 0.0) { 1.0 / parts.iter().map(|p| p.0 / [p.1, p.2][k]).sum::<f64>() } else { 0.0 };
    note(&mut img, "BulkBounds", vec![reuss(0), klo, khi, voigt(0)]);
    note(&mut img, "ShearBounds", vec![reuss(1), glo, ghi, voigt(1)]);
    note(&mut img, "SolverIterations", vec![iterations as f64]);
    note(&mut img, "SolverResidual", vec![residual]);
    Ok(DataSet::ImageData(img))
}

pub const GOAL_BULK: &str = "Bulk modulus";
pub const GOAL_SHEAR: &str = "Shear modulus";
pub const GOAL_CONDUCTIVITY: &str = "Conductivity";
pub const START_CENTRE: &str = "A soft centre";
pub const START_RANDOM: &str = "Random";

pub struct OptimizeParams {
    pub goal: String,
    pub fraction: f64,
    pub iterations: usize,
    /// Intermediate densities are made costly with this exponent.
    pub penalty: f64,
    /// Radius of the smoothing of the densities, in cells.
    pub radius: f64,
    /// The stiffness (or conductivity) of the void over that of the solid.
    pub contrast: f64,
    pub poisson: f64,
    pub start: String,
    pub seed: u64,
}

/// A periodic unit cell of the best bulk modulus, shear modulus or conductivity at a volume
/// fraction: every cell holds a density of material between 0 and 1, the property of the
/// periodic medium and its sensitivity to each density are computed, and material is moved
/// to where it serves most, the densities smoothed over a radius and pushed towards 0 and 1.
pub fn optimized_cell(g: &Grid, p: &OptimizeParams) -> Result<DataSet> {
    if !(p.fraction > 0.0 && p.fraction < 1.0) {
        bail!("the volume fraction must be between 0 and 1");
    }
    if g.len() > 2_000_000 {
        bail!("the optimisation is limited to 2 million cells");
    }
    let n = g.len();
    let planar = g.planar();
    let sp = Spectral::new(g.n);
    let contrast = p.contrast.clamp(1e-9, 0.5);
    let mut r = Rng::new(p.seed);
    let centre = g.size.map(|s| 0.5 * s);
    // An even density is a dead end (every cell then gains alike): the start has soft places,
    // one at the centre or a few at random, that take 0.4 of the box between them.
    let holes: Vec<[f64; 3]> = if p.start == START_RANDOM { (0..3).map(|_| g.random_point(&mut r)).collect() } else { vec![centre] };
    let share = 0.4 / holes.len() as f64;
    let reach_soft = if planar { (share * g.size[0] * g.size[1] / PI).sqrt() } else { (share * g.volume() * 0.75 / PI).cbrt() };
    let mut rho: Vec<f64> = (0..n)
        .map(|c| {
            let x = g.pos(c);
            let soft = holes.iter().any(|h| {
                let mut d = g.delta(x, *h);
                if planar {
                    d[2] = 0.0;
                }
                norm(d) < reach_soft
            });
            if soft {
                0.05
            } else {
                1.0
            }
        })
        .collect();
    let scale_to = |rho: &mut Vec<f64>| {
        let m = rho.iter().sum::<f64>() / n as f64;
        rho.iter_mut().for_each(|x| *x = (*x * p.fraction / m).clamp(1e-3, 1.0));
    };
    scale_to(&mut rho);
    // The smoothing: a cone of weights over the cells within the radius, through the faces.
    let reach = p.radius.max(0.0).ceil() as i64;
    let mut stencil: Vec<([i64; 3], f64)> = Vec::new();
    for dz in if planar { 0..=0 } else { -reach..=reach } {
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let w = p.radius.max(1e-9) - ((dx * dx + dy * dy + dz * dz) as f64).sqrt();
                if w > 0.0 || (dx, dy, dz) == (0, 0, 0) {
                    stencil.push(([dx, dy, dz], w.max(1e-9)));
                }
            }
        }
    }
    let total: f64 = stencil.iter().map(|s| s.1).sum();
    let smooth = |v: &[f64]| -> Vec<f64> {
        (0..n)
            .into_par_iter()
            .map(|c| {
                let q = g.ijk(c);
                stencil
                    .iter()
                    .map(|(o, w)| {
                        let i: [usize; 3] = std::array::from_fn(|k| (q[k] as i64 + o[k]).rem_euclid(g.n[k] as i64) as usize);
                        w * v[g.index(i)]
                    })
                    .sum::<f64>()
                    / total
            })
            .collect()
    };
    let conduct = p.goal == GOAL_CONDUCTIVITY;
    let (lam0, mu0) = lame(1.0, p.poisson.clamp(-0.9, 0.49));
    let loads: Vec<[f64; 6]> = match (p.goal.as_str(), planar) {
        (GOAL_SHEAR, true) => vec![unit_strain(5)],
        (GOAL_SHEAR, false) => vec![unit_strain(3), unit_strain(4), unit_strain(5)],
        (_, true) => vec![[1.0, 1.0, 0.0, 0.0, 0.0, 0.0]],
        _ => vec![[1.0, 1.0, 1.0, 0.0, 0.0, 0.0]],
    };
    let axes: Vec<usize> = (0..3).filter(|k| g.n[*k] > 1).collect();
    let mut guesses6: Vec<Option<Vec<[f64; 6]>>> = vec![None; loads.len()];
    let mut guesses3: Vec<Option<Vec<[f64; 3]>>> = vec![None; 3];
    let mut value = 0.0;
    let power = p.penalty.max(1.0);
    // The property and, for every cell, the energy a unit of stiffness there would hold.
    let mut evaluate = |soft: &[f64], tolerance: f64| -> (f64, Vec<f64>) {
        let stiff: Vec<f64> = soft.iter().map(|x| contrast + x.powf(power) * (1.0 - contrast)).collect();
        let mut energy = vec![0.0; n];
        let mut value = 0.0;
        if conduct {
            let law = |c: usize, e: &[f64; 3]| -> [f64; 3] { [stiff[c] * e[0], stiff[c] * e[1], stiff[c] * e[2]] };
            for &a in &axes {
                let mut load = [0.0; 3];
                load[a] = 1.0;
                let s = solve(&sp, &law, &gradient_rule, [1.0; 3], load, tolerance, 200, guesses3[a].as_deref());
                value += s.mean_flux[a] / axes.len() as f64;
                energy.par_iter_mut().zip(&s.field).for_each(|(w, e)| *w += dot(*e, *e));
                guesses3[a] = Some(s.field.iter().map(|e| sub(*e, load)).collect());
            }
        } else {
            let law = |c: usize, e: &[f64; 6]| stress(stiff[c] * lam0, stiff[c] * mu0, e);
            for (k, load) in loads.iter().enumerate() {
                let s = solve(&sp, &law, &strain_rule, STRAIN_WEIGHTS, *load, tolerance, 200, guesses6[k].as_deref());
                value += (0..6).map(|j| STRAIN_WEIGHTS[j] * load[j] * s.mean_flux[j]).sum::<f64>();
                energy.par_iter_mut().zip(&s.field).for_each(|(w, e)| {
                    let t = stress(lam0, mu0, e);
                    *w += (0..6).map(|j| STRAIN_WEIGHTS[j] * e[j] * t[j]).sum::<f64>();
                });
                guesses6[k] = Some(s.field.iter().map(|e| std::array::from_fn(|j| e[j] - load[j])).collect());
            }
        }
        (value, energy)
    };
    for _ in 0..p.iterations {
        let soft = smooth(&rho);
        let (v, energy) = evaluate(&soft, 1e-3);
        value = v;
        let raw: Vec<f64> = soft.iter().zip(&energy).map(|(x, w)| power * x.powf(power - 1.0) * (1.0 - contrast) * w).collect();
        let gain = smooth(&raw);
        // Each density is multiplied by the root of its gain over a price found so that the
        // amount of material stays the same; no density moves by more than 0.2 in a step.
        let (mut lo, mut hi) = (1e-12, gain.iter().cloned().fold(0.0, f64::max).max(1e-12) * 1e3);
        let step = |price: f64| -> Vec<f64> {
            rho.iter().zip(&gain).map(|(x, s)| (x * (s / price).max(0.0).sqrt()).clamp((x - 0.2).max(1e-3), (x + 0.2).min(1.0))).collect()
        };
        for _ in 0..60 {
            let price = (lo * hi).sqrt();
            if step(price).iter().sum::<f64>() / n as f64 > p.fraction {
                lo = price;
            } else {
                hi = price;
            }
        }
        rho = step((lo * hi).sqrt());
    }
    let soft = smooth(&rho);
    let phases = below_quantile(&soft.iter().map(|x| -*x as f32).collect::<Vec<f32>>(), p.fraction);
    let solid: Vec<f64> = phases.iter().map(|x| (*x == PHASE_SOLID) as u8 as f64).collect();
    let (final_value, _) = evaluate(&solid, 1e-4);
    let mut img = phase_image(g, phases, None);
    img.cell_data.insert(Array::scalars_f32("Density", soft.iter().map(|x| *x as f32).collect()));
    let dim = if planar { 2.0 } else { 3.0 };
    let (k_solid, g_solid) = (lam0 + 2.0 / 3.0 * mu0, mu0);
    let parts = [(1.0 - p.fraction, contrast * k_solid, contrast * g_solid), (p.fraction, k_solid, g_solid)];
    match p.goal.as_str() {
        GOAL_CONDUCTIVITY => {
            note(&mut img, "Conductivity", vec![final_value]);
            note(&mut img, "UpperBound", vec![conductivity_bounds(&[(1.0 - p.fraction, contrast), (p.fraction, 1.0)], dim).1]);
        }
        GOAL_SHEAR => {
            note(&mut img, "ShearModulus", vec![final_value / loads.len() as f64]);
            if !planar {
                note(&mut img, "UpperBound", vec![elastic_bounds(&parts).1 .1]);
            }
        }
        _ => {
            // The energy of a unit stretch along every axis is the bulk modulus times 9 (4 in the plane).
            note(&mut img, "BulkModulus", vec![final_value / (dim * dim)]);
            if !planar {
                note(&mut img, "UpperBound", vec![elastic_bounds(&parts).0 .1]);
            }
        }
    }
    note(&mut img, "ValueWithDensities", vec![value]);
    Ok(DataSet::ImageData(img))
}

/// The eigenvalues `w` of `K u = w M u` for Hermitian `K` and positive `M` (real and
/// imaginary parts, row by row), smallest first, through the real matrices twice as large.
fn generalized_eigenvalues(k: &[C], m: &[C], n: usize) -> Result<Vec<f64>> {
    let s = 2 * n;
    let embed = |a: &[C]| -> Vec<f64> {
        let mut out = vec![0.0; s * s];
        for i in 0..n {
            for j in 0..n {
                let v = a[i * n + j];
                out[i * s + j] = v[0];
                out[(i + n) * s + j + n] = v[0];
                out[i * s + j + n] = -v[1];
                out[(i + n) * s + j] = v[1];
            }
        }
        out
    };
    let mut l = embed(m);
    for j in 0..s {
        let mut d = l[j * s + j];
        for q in 0..j {
            d -= l[j * s + q] * l[j * s + q];
        }
        if !(d > 0.0) {
            bail!("the densities must be above 0 everywhere");
        }
        let d = d.sqrt();
        l[j * s + j] = d;
        for i in j + 1..s {
            let mut v = l[i * s + j];
            for q in 0..j {
                v -= l[i * s + q] * l[j * s + q];
            }
            l[i * s + j] = v / d;
        }
    }
    // Rows of the transpose of inv(L) A, then inv(L) of those: inv(L) A inv(L)^T.
    let half = |a: &[f64]| -> Vec<f64> {
        let cols: Vec<Vec<f64>> = (0..s)
            .into_par_iter()
            .map(|c| {
                let mut y = vec![0.0; s];
                for i in 0..s {
                    let mut v = a[i * s + c];
                    for q in 0..i {
                        v -= l[i * s + q] * y[q];
                    }
                    y[i] = v / l[i * s + i];
                }
                y
            })
            .collect();
        cols.concat()
    };
    let y = half(&embed(k));
    let mut b = half(&y);
    let ev = symmetric_eigenvalues(&mut b, s);
    Ok(ev.chunks(2).map(|p| 0.5 * (p[0] + p[1])).collect())
}

/// Wave Band Structure: the frequencies of the elastic waves that travel through the medium
/// repeated in every direction, along a path of wave vectors (from the centre of the zone of
/// one period to the middle of a face, of an edge, a corner and back), by an expansion of
/// the displacement on plane waves. A table with the path and one column per band.
#[allow(clippy::too_many_arguments)]
pub fn band_structure(
    ds: &DataSet,
    array: &str,
    youngs: &[f64],
    poisson: &[f64],
    density: &[f64],
    waves: usize,
    bands: usize,
    points: usize,
) -> Result<DataSet> {
    let (img, phases) = phases_of(ds, array)?;
    let (n, h) = cubic_cells(&img)?;
    if youngs.is_empty() || density.is_empty() || density.iter().any(|d| !(*d > 0.0)) || youngs.iter().any(|e| !(*e > 0.0)) {
        bail!("give a Young's modulus and a density above 0 for every phase");
    }
    let axes: Vec<usize> = (0..3).filter(|k| n[*k] > 1).collect();
    if axes.is_empty() {
        bail!("the grid has a single cell");
    }
    let reach = waves.max(1).min(axes.iter().map(|k| (n[*k] - 1) / 4).min().unwrap());
    if reach == 0 {
        bail!("the grid is too coarse: at least 5 cells along every axis");
    }
    let fft = Fft3::new(n);
    let total = fft.len() as f64;
    let field = |f: &(dyn Fn(i32) -> f64 + Sync)| -> Vec<C> {
        fft.forward(&phases.par_iter().map(|p| f(*p)).collect::<Vec<f64>>()).iter().map(|v| [v[0] / total, v[1] / total]).collect()
    };
    let nu = |p: i32| if poisson.is_empty() { 0.3 } else { of_phase(poisson, p) };
    let lam_hat = field(&|p| lame(of_phase(youngs, p), nu(p)).0);
    let mu_hat = field(&|p| lame(of_phase(youngs, p), nu(p)).1);
    let rho_hat = field(&|p| of_phase(density, p));
    let coefficient = |hat: &[C], m: [i64; 3]| -> C {
        let q: [usize; 3] = std::array::from_fn(|k| m[k].rem_euclid(n[k] as i64) as usize);
        hat[q[0] + n[0] * (q[1] + n[1] * q[2])]
    };
    let mut index: Vec<[i64; 3]> = Vec::new();
    let span = |k: usize| if n[k] > 1 { -(reach as i64)..=reach as i64 } else { 0..=0 };
    for z in span(2) {
        for y in span(1) {
            for x in span(0) {
                index.push([x, y, z]);
            }
        }
    }
    let npw = index.len();
    let dof = 3 * npw;
    let period: [f64; 3] = std::array::from_fn(|k| n[k] as f64 * h);
    let corner = |on: [bool; 3]| -> [f64; 3] { std::array::from_fn(|k| if on[k] && n[k] > 1 { PI / period[k] } else { 0.0 }) };
    let mut stops = vec![[0.0; 3], corner([axes[0] == 0, axes[0] == 1, axes[0] == 2])];
    if axes.len() > 1 {
        stops.push(corner([axes[..2].contains(&0), axes[..2].contains(&1), axes[..2].contains(&2)]));
    }
    if axes.len() > 2 {
        stops.push(corner([true; 3]));
    }
    stops.push([0.0; 3]);
    let per = points.max(2);
    let mut path: Vec<([f64; 3], f64)> = Vec::new();
    let mut along = 0.0;
    for w in stops.windows(2) {
        let len = norm(sub(w[1], w[0]));
        for q in 0..per {
            let t = q as f64 / per as f64;
            path.push((add(w[0], scale(sub(w[1], w[0]), t)), along + t * len));
        }
        along += len;
    }
    path.push(([0.0; 3], along));
    let rows: Vec<Result<Vec<f64>>> = path
        .par_iter()
        .map(|(k, _)| {
            let mut km = vec![[0.0; 2]; dof * dof];
            let mut mm = vec![[0.0; 2]; dof * dof];
            for (p, mp) in index.iter().enumerate() {
                let a: [f64; 3] = std::array::from_fn(|q| k[q] + 2.0 * PI * mp[q] as f64 / period[q]);
                for (q, mq) in index.iter().enumerate() {
                    let b: [f64; 3] = std::array::from_fn(|i| k[i] + 2.0 * PI * mq[i] as f64 / period[i]);
                    let d = [mp[0] - mq[0], mp[1] - mq[1], mp[2] - mq[2]];
                    let (l, m, rho) = (coefficient(&lam_hat, d), coefficient(&mu_hat, d), coefficient(&rho_hat, d));
                    let ab = dot(a, b);
                    for i in 0..3 {
                        mm[(3 * p + i) * dof + 3 * q + i] = rho;
                        for j in 0..3 {
                            let lw = a[i] * b[j];
                            let mw = a[j] * b[i] + if i == j { ab } else { 0.0 };
                            km[(3 * p + i) * dof + 3 * q + j] = [l[0] * lw + m[0] * mw, l[1] * lw + m[1] * mw];
                        }
                    }
                }
            }
            let ev = generalized_eigenvalues(&km, &mm, dof)?;
            Ok(ev.iter().take(bands.max(1)).map(|w| w.max(0.0).sqrt() / (2.0 * PI)).collect())
        })
        .collect();
    let mut freq: Vec<Vec<f64>> = Vec::new();
    for r in rows {
        freq.push(r?);
    }
    let shown = freq[0].len();
    let mut t = Table::default();
    t.columns.insert(Array::scalars_f64("Path", path.iter().map(|p| p.1).collect()));
    for (k, name) in ["Kx", "Ky", "Kz"].iter().enumerate() {
        t.columns.insert(Array::scalars_f64(*name, path.iter().map(|p| p.0[k]).collect()));
    }
    for b in 0..shown {
        t.columns.insert(Array::scalars_f64(format!("Band {}", b + 1), freq.iter().map(|f| f[b]).collect()));
    }
    // A gap: no wave at any frequency between the top of a band and the bottom of the next.
    let mut gaps = Vec::new();
    for b in 0..shown.saturating_sub(1) {
        let top = freq.iter().map(|f| f[b]).fold(0.0, f64::max);
        let bottom = freq.iter().map(|f| f[b + 1]).fold(f64::INFINITY, f64::min);
        if bottom > top * (1.0 + 1e-6) + 1e-12 {
            gaps.extend([top, bottom]);
        }
    }
    t.field_data.insert(Array::new("BandGaps", 2, ArrayData::F64(gaps)));
    t.field_data.insert(Array::scalars_f64("PlaneWaves", vec![npw as f64]));
    t.field_data.insert(Array::scalars_f64("PathStops", {
        let mut s = vec![0.0];
        let mut along = 0.0;
        for w in stops.windows(2) {
            along += norm(sub(w[1], w[0]));
            s.push(along);
        }
        s
    }));
    Ok(DataSet::Table(t))
}
