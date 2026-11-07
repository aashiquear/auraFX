//! Texture: pole figures, inverse pole figure densities, the orientation distribution function
//! (ODF) with its texture index, and the distribution of disorientation angles between random
//! orientations.
//!
//! Every density is in multiples of a random distribution (MRD): a kernel density estimate
//! with the Gaussian kernel `exp(-a² / 2σ²)` of the angle `a`, divided by the same estimate's
//! value for a uniform distribution, so a random texture gives 1 everywhere.
//! - Pole figure of a family {hkl}: the density, at a sample direction, of the sample
//!   coordinates `gᵀc` of the family's crystal directions `c` (all symmetry equivalents, both
//!   signs). The directions are first binned on a (polar angle, azimuth) grid of `bin` degrees
//!   and the kernel is applied to the bin centres.
//! - Inverse pole figure for a sample direction `d`: the density of the crystal directions
//!   `g d`, symmetrised over the crystal's symmetry and inversion, evaluated in the standard
//!   sector.
//! - ODF: the density at an orientation of the sample's orientations, the angle being the
//!   disorientation (the smallest of the symmetry-equivalent rotation angles).
//!
//! The uniform values are exact: for directions the mean of the kernel over the sphere; for
//! orientations the kernel averaged over the distribution of disorientation angles of random
//! orientations, which is `|G| (1 - cos ω) / π` below half the smallest symmetry rotation and
//! is integrated over rotation axes above it.

use crate::ipf::{ipf_sector, to_sector, IpfSector};
use crate::laue::Laue;
use crate::rotation::{conj, mul, rotate, Quat};
use rayon::prelude::*;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Mutex, OnceLock};

pub type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(v: V3) -> V3 {
    let n = dot(v, v).sqrt();
    v.map(|x| x / n)
}

/// The Gaussian kernel of an angle (radians).
pub fn kernel(angle: f64, sigma: f64) -> f64 {
    (-angle * angle / (2.0 * sigma * sigma)).exp()
}

// ------------------------------------------------------------------ directions

/// The distinct crystal directions of a family (every symmetry equivalent, both signs), unit
/// vectors rounded to 6 decimals.
pub fn pole_family(hkl: V3, laue: Laue) -> Vec<V3> {
    let v = unit(hkl);
    let mut out: Vec<V3> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for s in laue.operators() {
        let p = rotate(*s, v);
        for p in [p, p.map(|x| -x)] {
            let r = p.map(|x| (x * 1e6).round_ties_even() / 1e6);
            if seen.insert(r.map(|x| (x * 1e6).round() as i64)) {
                out.push(r);
            }
        }
    }
    out
}

/// Directions binned on a (polar angle, azimuth) grid: the centres of the bins that hold any,
/// with their total weights.
pub struct SphereBins {
    pub dirs: Vec<V3>,
    pub weights: Vec<f64>,
    pub total: f64,
}

/// Bin unit vectors on a grid of `bin_deg` degrees in polar angle (0..180) and azimuth (0..360).
pub fn bin_directions(dirs: &[V3], weights: Option<&[f64]>, bin_deg: f64) -> SphereBins {
    let nth = ((180.0 / bin_deg).round() as usize).max(1);
    let nph = ((360.0 / bin_deg).round() as usize).max(1);
    let edges = |n: usize, top: f64| -> Vec<f64> {
        let step = top / n as f64;
        let mut e: Vec<f64> = (0..=n).map(|i| i as f64 * step).collect();
        e[n] = top;
        e
    };
    let (et, ep) = (edges(nth, PI), edges(nph, 2.0 * PI));
    let find = |e: &[f64], x: f64| -> usize { (e.partition_point(|v| *v <= x)).saturating_sub(1).min(e.len() - 2) };
    let mut h = vec![0.0; nth * nph];
    for (i, d) in dirs.iter().enumerate() {
        let th = d[2].clamp(-1.0, 1.0).acos();
        let ph = d[1].atan2(d[0]).rem_euclid(2.0 * PI);
        h[find(&et, th) * nph + find(&ep, ph)] += weights.map(|w| w[i]).unwrap_or(1.0);
    }
    let mut out = SphereBins { dirs: Vec::new(), weights: Vec::new(), total: h.iter().sum() };
    for t in 0..nth {
        let tc = (t as f64 + 0.5) * PI / nth as f64;
        for p in 0..nph {
            let w = h[t * nph + p];
            if w > 0.0 {
                let pc = (p as f64 + 0.5) * 2.0 * PI / nph as f64;
                out.dirs.push([tc.sin() * pc.cos(), tc.sin() * pc.sin(), tc.cos()]);
                out.weights.push(w);
            }
        }
    }
    out
}

/// The mean of the kernel over the sphere (trapezoidal rule on 4001 points).
pub fn uniform_sphere_expectation(sigma: f64) -> f64 {
    let n = 4001;
    let t: Vec<f64> = (0..n).map(|i| i as f64 * (PI / (n - 1) as f64)).collect();
    let y: Vec<f64> = t.iter().map(|t| kernel(*t, sigma) * t.sin()).collect();
    0.5 * (1..n).map(|i| (y[i] + y[i - 1]) / 2.0 * (t[i] - t[i - 1])).sum::<f64>()
}

/// The density (MRD) of binned directions at the directions `at`.
pub fn sphere_density(bins: &SphereBins, at: &[V3], sigma: f64) -> Vec<f64> {
    let e = uniform_sphere_expectation(sigma);
    at.par_iter()
        .map(|y| {
            let acc: f64 = bins.dirs.iter().zip(&bins.weights).map(|(c, w)| kernel(dot(*y, *c).clamp(-1.0, 1.0).acos(), sigma) * w).sum();
            acc / bins.total / e
        })
        .collect()
}

/// Pole figure (MRD) of the family `hkl` at the sample directions `at`, from orientations `q`
/// (one class) with optional weights. Angles in radians, `bin_deg` in degrees.
pub fn pole_figure(q: &[Quat], weights: Option<&[f64]>, hkl: V3, laue: Laue, at: &[V3], sigma: f64, bin_deg: f64) -> Vec<f64> {
    let fam = pole_family(hkl, laue);
    let mut dirs = Vec::with_capacity(q.len() * fam.len());
    let mut w = Vec::new();
    for (i, g) in q.iter().enumerate() {
        let gi = conj(*g);
        for c in &fam {
            dirs.push(rotate(gi, *c));
            if let Some(ws) = weights {
                w.push(ws[i]);
            }
        }
    }
    let bins = bin_directions(&dirs, weights.map(|_| w.as_slice()), bin_deg);
    sphere_density(&bins, at, sigma)
}

/// Inverse pole figure density (MRD) for sample direction `d` at the crystal directions `at`
/// (normally points of the standard sector).
pub fn inverse_pole_figure(q: &[Quat], weights: Option<&[f64]>, d: V3, laue: Laue, at: &[V3], sigma: f64, bin_deg: f64) -> Vec<f64> {
    let d = unit(d);
    let dirs: Vec<V3> = q
        .iter()
        .map(|g| {
            let c = rotate(*g, d);
            let (eta, chi) = to_sector(c, laue).unwrap_or((0.0, 0.0));
            [chi.sin() * eta.cos(), chi.sin() * eta.sin(), chi.cos()]
        })
        .collect();
    let bins = bin_directions(&dirs, weights, bin_deg);
    // Every symmetry copy of every bin, keeping those near the points asked for.
    let mut centre = [0.0; 3];
    for a in at {
        for k in 0..3 {
            centre[k] += a[k];
        }
    }
    let centre = if dot(centre, centre) > 0.0 { unit(centre) } else { [0.0, 0.0, 1.0] };
    let reach = at.iter().map(|a| dot(*a, centre).clamp(-1.0, 1.0).acos()).fold(0.0, f64::max) + 6.0 * sigma;
    let ops = laue.operators();
    let mut copies = SphereBins { dirs: Vec::new(), weights: Vec::new(), total: bins.total };
    for (c, w) in bins.dirs.iter().zip(&bins.weights) {
        for s in ops {
            let p = rotate(*s, *c);
            for p in [p, p.map(|x| -x)] {
                if reach >= PI || dot(p, centre).clamp(-1.0, 1.0).acos() <= reach {
                    copies.dirs.push(p);
                    copies.weights.push(*w);
                }
            }
        }
    }
    let n = (2 * ops.len()) as f64;
    sphere_density(&copies, at, sigma).into_iter().map(|v| v / n).collect()
}

/// The standard sector of a class as a grid of crystal directions: rows from the pole (row 0,
/// all points at the pole) to the sector's edge, `n + 1` points per row across its azimuth.
pub fn sector_grid(sector: &IpfSector, n: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity((n + 1) * (n + 1));
    for r in 0..=n {
        for k in 0..=n {
            let eta = sector.eta_min + (sector.eta_max - sector.eta_min) * k as f64 / n as f64;
            out.push((eta, sector.chi_max(eta) * r as f64 / n as f64));
        }
    }
    out
}

/// The sector of a class (re-exported for callers building grids).
pub fn sector_of(laue: Laue) -> IpfSector {
    ipf_sector(laue)
}

// ------------------------------------------------------------------ orientations

/// Sum over `samples` of the kernel of the disorientation angle to `g`, divided by the count.
fn kernel_mean_one(g: Quat, expanded: &[Quat], nops: usize, sigma: f64, cubic: bool) -> f64 {
    let n = expanded.len() / nops.max(1);
    let mut tot = 0.0;
    if cubic {
        // Heinz & Neumann: for the cubic group the largest |w| over the 24 equivalents is the
        // largest of a0, (a0 + a1) / √2 and (a0 + a1 + a2 + a3) / 2, components sorted by size.
        for s in expanded {
            let d = mul(g, conj(*s));
            let mut a = d.map(f64::abs);
            a.sort_unstable_by(|x, y| y.total_cmp(x));
            let c = a[0].max((a[0] + a[1]) / std::f64::consts::SQRT_2).max(0.5 * (a[0] + a[1] + a[2] + a[3]));
            let om = 2.0 * c.clamp(-1.0, 1.0).acos();
            tot += kernel(om, sigma);
        }
    } else {
        for chunk in expanded.chunks(nops) {
            let mut c: f64 = 0.0;
            for s in chunk {
                c = c.max((g[0] * s[0] + g[1] * s[1] + g[2] * s[2] + g[3] * s[3]).abs());
            }
            tot += kernel(2.0 * c.min(1.0).acos(), sigma);
        }
    }
    tot / n as f64
}

/// The mean kernel of the disorientation from every grid orientation to the samples.
pub fn odf_kernel_means(grid: &[Quat], samples: &[Quat], sigma: f64, laue: Laue) -> Vec<f64> {
    let cubic = laue == Laue::CubicHigh;
    let (expanded, nops) = if cubic {
        (samples.to_vec(), 1)
    } else {
        let ops = laue.operators();
        (samples.iter().flat_map(|q| ops.iter().map(move |s| mul(*s, *q))).collect::<Vec<_>>(), ops.len())
    };
    grid.par_iter().map(|g| kernel_mean_one(*g, &expanded, nops, sigma, cubic)).collect()
}

/// The distribution of disorientation angles between random orientations of a class: the
/// density (per radian) on bins of 0.1° above `start`, below which it is `|G| (1 - cos ω) / π`.
pub struct RandomDisorientations {
    pub nops: usize,
    pub start: f64,
    pub width: f64,
    pub density: Vec<f64>,
}

impl RandomDisorientations {
    /// The probability that the disorientation lies between `lo` and `hi` (radians).
    pub fn fraction(&self, lo: f64, hi: f64) -> f64 {
        let g = self.nops as f64;
        let analytic = |a: f64| g * (a - a.sin()) / PI;
        let (alo, ahi) = (lo.min(self.start), hi.min(self.start));
        let mut f = if ahi > alo { analytic(ahi) - analytic(alo) } else { 0.0 };
        for (k, d) in self.density.iter().enumerate() {
            let b0 = self.start + k as f64 * self.width;
            let b1 = b0 + self.width;
            let o = hi.min(b1) - lo.max(b0);
            if o > 0.0 {
                f += d * o;
            }
        }
        f
    }
    /// The density (per radian) at angle `w`.
    pub fn density_at(&self, w: f64) -> f64 {
        if w < self.start {
            return self.nops as f64 * (1.0 - w.cos()) / PI;
        }
        let k = ((w - self.start) / self.width) as usize;
        self.density.get(k).copied().unwrap_or(0.0)
    }
    pub fn max_angle(&self) -> f64 {
        match self.density.iter().rposition(|d| *d > 0.0) {
            Some(k) => self.start + (k + 1) as f64 * self.width,
            None => self.start,
        }
    }
}

/// The disorientation-angle distribution of random orientations of `laue` (computed once).
pub fn random_disorientations(laue: Laue) -> &'static RandomDisorientations {
    static CACHE: OnceLock<Mutex<HashMap<u32, &'static RandomDisorientations>>> = OnceLock::new();
    let m = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(r) = m.lock().unwrap().get(&laue.code()) {
        return r;
    }
    let ops = laue.operators();
    // Below half the smallest symmetry rotation every rotation is its own disorientation.
    let smallest = ops.iter().map(|s| 2.0 * s[0].abs().min(1.0).acos()).filter(|a| *a > 1e-9).fold(PI, f64::min);
    let start = (smallest / 2.0).min(PI);
    let width = 0.1f64.to_radians();
    let nb = ((PI - start) / width).ceil() as usize;
    // Rotation axes: a Fibonacci sphere.
    let na = 40_000;
    let golden = PI * (3.0 - 5f64.sqrt());
    let axes: Vec<V3> = (0..na)
        .map(|i| {
            let z = 1.0 - (2.0 * i as f64 + 1.0) / na as f64;
            let r = (1.0 - z * z).sqrt();
            let p = golden * i as f64;
            [r * p.cos(), r * p.sin(), z]
        })
        .collect();
    let g = ops.len() as f64;
    let density: Vec<f64> = (0..nb)
        .into_par_iter()
        .map(|k| {
            let w = (start + (k as f64 + 0.5) * width).min(PI);
            let (c, s) = ((w / 2.0).cos(), (w / 2.0).sin());
            let inside = axes
                .iter()
                .filter(|a| {
                    let q = [c, s * a[0], s * a[1], s * a[2]];
                    ops.iter().all(|o| (o[0] * q[0] - o[1] * q[1] - o[2] * q[2] - o[3] * q[3]).abs() <= c + 1e-12)
                })
                .count();
            g * (1.0 - w.cos()) / PI * inside as f64 / na as f64
        })
        .collect();
    let r: &'static RandomDisorientations = Box::leak(Box::new(RandomDisorientations { nops: ops.len(), start, width, density }));
    m.lock().unwrap().insert(laue.code(), r);
    r
}

/// The mean kernel of the disorientation from a fixed orientation to random ones.
pub fn uniform_odf_expectation(sigma: f64, laue: Laue) -> f64 {
    let r = random_disorientations(laue);
    let g = r.nops as f64;
    // Simpson's rule on the analytic part, then the bins.
    let n = 4000;
    let h = r.start / n as f64;
    let f = |w: f64| kernel(w, sigma) * g * (1.0 - w.cos()) / PI;
    let mut s = f(0.0) + f(r.start);
    for i in 1..n {
        s += f(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    let mut e = s * h / 3.0;
    for (k, d) in r.density.iter().enumerate() {
        let w = r.start + (k as f64 + 0.5) * r.width;
        e += kernel(w.min(PI), sigma) * d * r.width;
    }
    e
}

/// The ODF (MRD) at the grid orientations.
pub fn odf_values(grid: &[Quat], samples: &[Quat], sigma: f64, laue: Laue) -> Vec<f64> {
    let e = uniform_odf_expectation(sigma, laue);
    odf_kernel_means(grid, samples, sigma, laue).into_iter().map(|v| v / e).collect()
}

/// The Bunge-angle region that holds whole copies of orientation space for a class: φ2 below
/// the period of the class's rotations about z, and Φ up to 90° when a two-fold axis lies
/// perpendicular to z (else 180°). Returns (φ2 period, Φ max), radians.
pub fn euler_region(laue: Laue) -> (f64, f64) {
    let ops = laue.operators();
    let about_z = ops.iter().filter(|s| s[1].abs() < 1e-9 && s[2].abs() < 1e-9).count();
    // Each rotation appears once (w >= 0) or as a pair (q, -q); count distinct angles.
    let mut angles: Vec<i64> = ops
        .iter()
        .filter(|s| s[1].abs() < 1e-9 && s[2].abs() < 1e-9)
        .map(|s| ((2.0 * s[3].atan2(s[0])).rem_euclid(2.0 * PI) * 1e6).round() as i64 % (2.0 * PI * 1e6).round() as i64)
        .collect();
    angles.sort_unstable();
    angles.dedup();
    let n = angles.len().max(1).min(about_z.max(1));
    let perp = ops.iter().any(|s| s[0].abs() < 1e-9 && s[3].abs() < 1e-9);
    (2.0 * PI / n as f64, if perp { PI / 2.0 } else { PI })
}

/// The Bunge angles of a φ2 section: φ1 from 0 to 360° and Φ from 0 to `phi_max`, both
/// inclusive in steps of `step` (radians); rows of constant Φ.
pub fn section_grid(phi2: f64, step: f64, phi_max: f64) -> (Vec<[f64; 3]>, usize, usize) {
    let n1 = (2.0 * PI / step).round() as usize + 1;
    let n2 = (phi_max / step).round() as usize + 1;
    let mut out = Vec::with_capacity(n1 * n2);
    for j in 0..n2 {
        for i in 0..n1 {
            out.push([i as f64 * step, j as f64 * step, phi2]);
        }
    }
    (out, n1, n2)
}

/// Texture index J (the mean of f² over orientation space) and the largest ODF value, from the
/// midpoints of a grid of `step` radians over the region of [`euler_region`], weighted by
/// sin Φ.
pub fn texture_index(samples: &[Quat], sigma: f64, laue: Laue, step: f64) -> (f64, f64) {
    let (period, phi_max) = euler_region(laue);
    let n1 = (2.0 * PI / step).round().max(1.0) as usize;
    let n2 = (phi_max / step).round().max(1.0) as usize;
    let n3 = (period / step).round().max(1.0) as usize;
    let (s1, s2, s3) = (2.0 * PI / n1 as f64, phi_max / n2 as f64, period / n3 as f64);
    let mut grid = Vec::with_capacity(n1 * n2 * n3);
    let mut w = Vec::with_capacity(n1 * n2 * n3);
    for k in 0..n3 {
        for j in 0..n2 {
            for i in 0..n1 {
                let e = [(i as f64 + 0.5) * s1, (j as f64 + 0.5) * s2, (k as f64 + 0.5) * s3];
                grid.push(crate::rotation::euler_to_quat(e));
                w.push(e[1].sin());
            }
        }
    }
    let f = odf_values(&grid, samples, sigma, laue);
    let sw: f64 = w.iter().sum();
    let j = f.iter().zip(&w).map(|(f, w)| f * f * w).sum::<f64>() / sw;
    (j, f.iter().cloned().fold(0.0, f64::max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_disorientations_integrate_to_one() {
        for l in Laue::ALL {
            let r = random_disorientations(l);
            let total = r.fraction(0.0, PI);
            assert!((total - 1.0).abs() < 2e-3, "{l:?} {total}");
        }
        // Cubic: the largest disorientation is 62.8 degrees.
        let m = random_disorientations(Laue::CubicHigh).max_angle().to_degrees();
        assert!((m - 62.8).abs() < 0.15, "{m}");
        let (p, f) = euler_region(Laue::CubicHigh);
        assert!((p - PI / 2.0).abs() < 1e-12 && (f - PI / 2.0).abs() < 1e-12);
        let (p, f) = euler_region(Laue::HexagonalLow);
        assert!((p - PI / 3.0).abs() < 1e-12 && (f - PI).abs() < 1e-12);
        assert_eq!(euler_region(Laue::Triclinic), (2.0 * PI, PI));
        assert_eq!(pole_family([1.0, 0.0, 0.0], Laue::CubicHigh).len(), 6);
        assert_eq!(pole_family([1.0, 1.0, 1.0], Laue::CubicHigh).len(), 8);
        assert_eq!(pole_family([1.0, 1.0, 0.0], Laue::CubicHigh).len(), 12);
    }
}
