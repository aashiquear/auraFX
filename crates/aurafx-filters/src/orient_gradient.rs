//! Orientation gradients inside grains: a synthetic microstructure has one orientation per
//! grain; grains of additively manufactured material turn slowly as they grow, carry a smooth
//! orientation spread and are divided into sub-grain cells by low-angle boundaries. This
//! rewrites the per-cell orientations of a labelled volume with such a rotation field:
//!
//! - a steady turn about one axis per grain, in proportion to the distance along a direction
//!   from the grain's reference point (its centroid or its lowest point);
//! - a smooth random turn (a few random waves per grain, with a correlation length);
//! - sub-grain cells (a jittered lattice of cell centres, each cell of each grain turned by
//!   its own small rotation);
//! - optionally more of all three in larger grains, and a cap on the total turn.
//!
//! The rotation is applied in the sample frame (on the right of the orientation, which maps
//! sample to crystal directions), so the misorientation of a cell from where it started is
//! the angle of the rotation whatever the crystal symmetry.

use crate::features::{image, Vox};
use crate::synthetic::{encode_like, feature_ids, orientation_target, replace, Draws, Rng};
use crate::Association;
use anyhow::{anyhow, bail, Result};
use aurafx_crystal::{self as cx, Quat};
use aurafx_data::*;
use rayon::prelude::*;

pub const AXIS_RANDOM: &str = "Random";
pub const AXIS_PERPENDICULAR: &str = "Perpendicular to the direction";
pub const AXIS_FIXED: &str = "Fixed";
pub const REF_CENTROID: &str = "Centroid";
pub const REF_LOWEST: &str = "Lowest point";

/// Waves of the smooth random turn of one grain.
const MODES: usize = 6;

#[derive(Clone, Debug)]
pub struct GradientParams {
    /// Degrees per unit length along `direction`.
    pub rate: f64,
    pub direction: [f64; 3],
    /// Log-standard deviation of the grains' rates about `rate`.
    pub rate_spread: f64,
    pub axis: String,
    pub fixed_axis: [f64; 3],
    pub reference: String,
    /// Root-mean-square angle of the smooth random turn, degrees.
    pub amplitude: f64,
    /// Its correlation length (0: half the grain's diameter).
    pub correlation_length: f64,
    /// Size of the sub-grain cells (0: none).
    pub cell_size: f64,
    /// Mean misorientation between neighbouring cells, degrees.
    pub cell_angle: f64,
    /// Rate and amplitude scale with (grain diameter / median diameter) to this power.
    pub size_exponent: f64,
    /// Largest turn from the starting orientation, degrees (0: no limit).
    pub cap: f64,
    pub seed: u64,
}

impl GradientParams {
    /// The default model with a given rate, as the synthetic sources apply it.
    pub fn model(rate: f64, seed: u64) -> GradientParams {
        GradientParams {
            rate,
            direction: [0.0, 0.0, 1.0],
            rate_spread: 0.5,
            axis: AXIS_RANDOM.into(),
            fixed_axis: [1.0, 0.0, 0.0],
            reference: REF_CENTROID.into(),
            amplitude: 0.5,
            correlation_length: 0.0,
            cell_size: 0.0,
            cell_angle: 1.0,
            size_exponent: 0.0,
            cap: 0.0,
            seed,
        }
    }
    fn is_off(&self) -> bool {
        self.rate == 0.0 && self.amplitude <= 0.0 && (self.cell_size <= 0.0 || self.cell_angle <= 0.0)
    }
}

/// splitmix64: one well-mixed word from another.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn mix4(seed: u64, a: i64, b: i64, c: i64, d: i64) -> u64 {
    mix(mix(mix(mix(mix(seed) ^ a as u64) ^ b as u64) ^ c as u64) ^ d as u64)
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (n > 1e-12 && n.is_finite()).then(|| v.map(|x| x / n))
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn random_unit(r: &mut dyn Draws) -> [f64; 3] {
    let z = r.uniform(-1.0, 1.0);
    let phi = r.uniform(0.0, std::f64::consts::TAU);
    let s = (1.0 - z * z).sqrt();
    [s * phi.cos(), s * phi.sin(), z]
}

/// The rotation of a rotation vector (axis times angle in radians).
fn from_vector(w: [f64; 3]) -> Quat {
    cx::axis_angle_to_quat(w, (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt())
}

struct Grain {
    centroid: [f64; 3],
    /// Reference coordinate along the direction.
    s_ref: f64,
    /// Radians per unit length.
    rate: f64,
    axis: [f64; 3],
    /// (wave vector, phase, rotation-vector amplitude in radians) of each wave.
    waves: [([f64; 3], f64, [f64; 3]); MODES],
    /// Standard deviation of each component of a cell's rotation vector, radians.
    cell_sd: f64,
}

/// Orientation Gradient: the orientations of every cell inside a feature turned by the
/// rotation field of `p` (see the module's description). With every part off the data is
/// returned as it is.
pub fn orientation_gradient(ds: &DataSet, orientations: &str, ids_name: &str, p: &GradientParams) -> Result<DataSet> {
    if p.is_off() {
        return Ok(ds.clone());
    }
    let img = image(ds)?;
    let v = Vox::of(img);
    let (assoc, a) = orientation_target(ds, orientations)?;
    if assoc != Association::Cells {
        bail!("the orientations must be cell data");
    }
    let repr = crate::crystal::Repr::guess(a.ncomp).ok_or_else(|| anyhow!("{} is not an orientation array", a.name))?;
    let n = a.len();
    let ids = feature_ids(ds, ids_name, assoc, n)?;
    let dir = unit(p.direction).ok_or_else(|| anyhow!("the direction has no length"))?;
    let fixed = match p.axis.as_str() {
        AXIS_FIXED => Some(unit(p.fixed_axis).ok_or_else(|| anyhow!("the fixed axis has no length"))?),
        _ => None,
    };
    let nf = ids.iter().copied().max().unwrap_or(0).max(0) as usize + 1;
    // Per grain: cells, centroid, lowest coordinate along the direction.
    let mut count = vec![0usize; nf];
    let mut sum = vec![[0.0f64; 3]; nf];
    let mut low = vec![f64::INFINITY; nf];
    for (c, &f) in ids.iter().enumerate() {
        if f > 0 {
            let x = v.center(c);
            let f = f as usize;
            count[f] += 1;
            for k in 0..3 {
                sum[f][k] += x[k];
            }
            low[f] = low[f].min(dot(x, dir));
        }
    }
    let axes: Vec<usize> = (0..3).filter(|k| !v.flat[*k] && v.n[*k] > 1).collect();
    let dim = axes.len().max(1) as f64;
    let cell_measure: f64 = axes.iter().map(|k| v.sp[*k]).product();
    let diameter: Vec<f64> = count.iter().map(|c| (*c as f64 * cell_measure).powf(1.0 / dim)).collect();
    let mut present: Vec<f64> = (1..nf).filter(|f| count[*f] > 0).map(|f| diameter[f]).collect();
    if present.is_empty() {
        return Ok(ds.clone());
    }
    present.sort_by(|x, y| x.total_cmp(y));
    let median = present[present.len() / 2];
    let grains: Vec<Option<Grain>> = (0..nf)
        .map(|f| {
            if f == 0 || count[f] == 0 {
                return None;
            }
            let mut r = Rng::new(mix(p.seed ^ mix(f as u64)));
            let centroid = sum[f].map(|x| x / count[f] as f64);
            let scale = if p.size_exponent != 0.0 { (diameter[f] / median).powf(p.size_exponent) } else { 1.0 };
            let spread = if p.rate_spread > 0.0 { (p.rate_spread * r.std_normal().clamp(-3.0, 3.0)).exp() } else { 1.0 };
            let any = random_unit(&mut r);
            let axis = match (p.axis.as_str(), fixed) {
                (_, Some(a)) => a,
                (AXIS_PERPENDICULAR, _) => {
                    let d = dot(any, dir);
                    unit(std::array::from_fn(|k| any[k] - d * dir[k])).unwrap_or_else(|| {
                        // The draw fell along the direction: any perpendicular will do.
                        let o = if dir[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
                        let d = dot(o, dir);
                        unit(std::array::from_fn(|k| o[k] - d * dir[k])).unwrap()
                    })
                }
                _ => any,
            };
            let length = if p.correlation_length > 0.0 { p.correlation_length } else { 0.5 * diameter[f] };
            let amp = p.amplitude.max(0.0).to_radians() * scale * (2.0 / MODES as f64).sqrt();
            let waves = std::array::from_fn(|_| {
                let k: [f64; 3] = std::array::from_fn(|a| if axes.contains(&a) { r.std_normal() / length } else { 0.0 });
                let phase = r.uniform(0.0, std::f64::consts::TAU);
                let b = random_unit(&mut r);
                (k, phase, b.map(|x| x * amp))
            });
            Some(Grain {
                centroid,
                s_ref: if p.reference == REF_LOWEST { low[f] } else { dot(centroid, dir) },
                rate: p.rate.to_radians() * spread * scale,
                axis,
                waves,
                // Two cells differ by a vector with components N(0, 2 sd^2): mean length 2.257 sd.
                cell_sd: if p.cell_size > 0.0 { p.cell_angle.max(0.0).to_radians() * scale / 2.2568 } else { 0.0 },
            })
        })
        .collect();
    let cap = p.cap.max(0.0).to_radians();
    let cell_seed = mix(p.seed ^ 0x5EED_CE11);
    let qs: Vec<Quat> = (0..n)
        .into_par_iter()
        .map(|c| {
            let q = repr.decode_values(&a, c);
            let Some(g) = (ids[c] > 0).then(|| grains[ids[c] as usize].as_ref()).flatten() else { return q };
            let x = v.center(c);
            let mut rot = cx::axis_angle_to_quat(g.axis, g.rate * (dot(x, dir) - g.s_ref));
            if p.amplitude > 0.0 {
                let rel: [f64; 3] = std::array::from_fn(|k| x[k] - g.centroid[k]);
                let mut w = [0.0; 3];
                for (k, phase, b) in &g.waves {
                    let s = (dot(*k, rel) + phase).cos();
                    for a in 0..3 {
                        w[a] += b[a] * s;
                    }
                }
                rot = cx::mul(rot, from_vector(w));
            }
            if g.cell_sd > 0.0 {
                // The nearest centre of a lattice of cubes, one jittered centre per cube.
                let base: [i64; 3] = std::array::from_fn(|k| (x[k] / p.cell_size).floor() as i64);
                let mut best = (f64::INFINITY, [0i64; 3]);
                for dz in -1..=1 {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let cube = [base[0] + dx, base[1] + dy, base[2] + dz];
                            let h = mix4(cell_seed, cube[0], cube[1], cube[2], 0);
                            let jit = [(h & 0xFFFF) as f64, ((h >> 16) & 0xFFFF) as f64, ((h >> 32) & 0xFFFF) as f64].map(|u| u / 65536.0);
                            let d2: f64 = (0..3).map(|k| if axes.contains(&k) { (x[k] - (cube[k] as f64 + jit[k]) * p.cell_size).powi(2) } else { 0.0 }).sum();
                            if d2 < best.0 {
                                best = (d2, cube);
                            }
                        }
                    }
                }
                let mut r = Rng::new(mix4(cell_seed, best.1[0], best.1[1], best.1[2], ids[c] as i64));
                let w = [r.std_normal() * g.cell_sd, r.std_normal() * g.cell_sd, r.std_normal() * g.cell_sd];
                rot = cx::mul(rot, from_vector(w));
            }
            if cap > 0.0 {
                // The angle saturates smoothly at the cap; the axis stays.
                let rot_p = cx::positive(cx::normalize(rot));
                let s = (rot_p[1] * rot_p[1] + rot_p[2] * rot_p[2] + rot_p[3] * rot_p[3]).sqrt();
                let angle = 2.0 * s.atan2(rot_p[0]);
                rot = cx::axis_angle_to_quat([rot_p[1], rot_p[2], rot_p[3]], cap * (angle / cap).tanh());
            }
            cx::normalize(cx::mul(q, rot))
        })
        .collect();
    replace(ds, assoc, encode_like(&a, &qs)?)
}
